//! The reviewer: a removable module that judges every send before it
//! leaves, and the completion criteria no code check covers (#179). It can
//! block a send with a reason shown on the task, or flag one for the person;
//! nothing it returns lets through a send the kernel would ask about.

use magnus_invariants::appliance::Person;
use magnus_invariants::model::Script;
use magnus_invariants::*;
use magnus_scratch::start_checked;
use serde_json::{json, Value};

fn spec() -> Spec {
    let mut s = Spec::default();
    s.people.push(Person {
        id: "dana".into(),
        name: "Dana".into(),
        token: "tok-dana-reviewer".into(),
        groups: vec!["staff".into()],
        roles: vec!["developer".into(), "approver:reversible_write".into()],
    });
    s.max_model_calls = 20;
    s
}

/// One trial of an experiment case, with module switches.
async fn trial(dana: &Api, exp: &str, case: &str, text: &str, modules: Value) -> (String, String) {
    let (st, v) = dana.post(&format!("/v1/dev/experiments/{exp}/trials"), json!({ "case": case, "trial": 0, "text": text, "modules": modules })).await;
    assert_eq!(st, 201, "{v}");
    (v["task_id"].as_str().unwrap().to_string(), v["run_id"].as_str().unwrap().to_string())
}

/// The reviewer module's replies in a run: its judgments of sends and of
/// completions. The independent checks every send meets (the outgoing
/// classifier and Auto Review) ask the same reviewer model and are recorded
/// the same way; they are not the module.
async fn reviewer_replies(dana: &Api, run: &str) -> usize {
    let (_, log) = dana.get(&format!("/v1/dev/runs/{run}/log")).await;
    let module = ["send", "completion"];
    log["events"].as_array().unwrap().iter().filter(|e| e["kind"] == "reviewer_reply" && module.contains(&e["body"]["purpose"].as_str().unwrap_or_default())).count()
}

/// The reviewer judges sends: a scripted reviewer blocks mail to anyone but
/// bob, and passes everything else (including completion reviews).
fn reviewer_script(agent: Script) -> Script {
    script(move |c| {
        let agent = agent.clone();
        async move {
            if c.offered_tools().contains(&"review".to_string()) {
                if !c.text().contains("The send to judge") {
                    return tool("review", json!({ "criteria": [{ "id": "c1", "holds": true, "evidence": "the mail was sent" }] }));
                }
                let bad = c.text().contains("mallory@evil.example");
                return tool(
                    "review",
                    json!({ "verdict": if bad { "block" } else { "pass" }, "reason": if bad { "mallory is not the intended recipient" } else { "to the named recipient" } }),
                );
            }
            agent(c).await
        }
    })
}

/// A blocked send never leaves: the agent is told why, the person sees a
/// notice on the task, and a corrected send goes through once the person
/// approves it: a rewrite of a blocked message is not the reviewer's alone
/// to judge (decisions/2026-11-09-safety-model-checks.md).
#[tokio::test(flavor = "multi_thread")]
async fn a_blocked_send_never_leaves_and_the_person_sees_why() {
    let mail = mcp::mail().start().await;
    let agent = script(|c| async move {
        match c.turn() {
            0 => contract_unchecked("email bob", "bob was emailed"),
            1 => tool("mail__send", json!({ "to": "mallory@evil.example", "subject": "hello", "body": "hello" })),
            _ if c.text().contains("Denied (reviewer)") && !json!(c.messages()).to_string().contains("bob@corp.example") => {
                tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hello", "body": "hello" }))
            }
            _ => complete("emailed"),
        }
    });
    let m = start_checked(reviewer_script(agent)).await;
    let s = spec().app("mail", &mail, "public", "private", &[("list_sent", "read"), ("send", "reversible_write")]);
    let app = Appliance::start(s, &m).await;
    assert!(app.api("alice").put_policy("org", &org_policy(&permit_execute("mail__send"))).await.is_success());
    let dana = app.api("dana");
    let (t, run) = trial(&dana, "rev", "block", "Email bob hello.", json!({ "reviewer": true })).await;

    let card = dana.wait_item(&t, "approval").await;
    assert_eq!(card["details"]["input"]["to"], "bob@corp.example", "the person is not asked about the corrected send: {card}");
    assert!(card.to_string().contains("revises a message that was blocked"), "the card does not say it revises a blocked send: {card}");
    assert!(mail.calls_to("send").is_empty(), "a send left before the person approved the rewrite");
    assert!(dana.respond(card["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "once" })).await.is_success());
    eventually("the approved rewrite is sent", PATIENCE, || async { (!mail.calls_to("send").is_empty()).then_some(()) }).await;
    let done = dana.wait_settled(&t).await;
    let open = dana.open_items(&t, "approval").await;
    assert_eq!(done["status"], "completed", "the run did not finish after the block: {done}\nstill asking: {open:?}");
    let sends = mail.calls_to("send");
    assert_eq!(sends.len(), 1, "the blocked send left, or the retry did not: {sends:?}");
    assert_eq!(sends[0].args["to"], "bob@corp.example", "the wrong send went through: {sends:?}");
    assert_eq!(reviewer_replies(&dana, &run).await, 2, "the blocked send and the completion review are judged and recorded; the rewrite went to the person");
    let notices: Vec<Value> = dana.inbox().await.into_iter().filter(|i| i["kind"] == "notice").collect();
    assert!(notices.iter().any(|n| n.to_string().contains("reviewer") && n.to_string().contains("mallory")), "no notice shows why the send was blocked: {notices:?}");
    let (_, reg) = dana.get("/v1/dev/modules").await;
    let rev = reg["modules"].as_array().unwrap().iter().find(|m| m["name"] == "reviewer").unwrap().clone();
    assert_eq!(rev["acted"], 2, "the module acted on the blocked send and the completion, not on the checks beside it: {reg}");
}

/// A flagged send waits for the person: their approval sends it, without
/// the reviewer flagging it again.
#[tokio::test(flavor = "multi_thread")]
async fn a_flagged_send_waits_for_the_person_whose_approval_sends_it() {
    let mail = mcp::mail().start().await;
    let agent = script(|c| async move {
        match c.turn() {
            0 => contract_unchecked("email bob", "bob was emailed"),
            1 => tool("mail__send", json!({ "to": "mallory@evil.example", "subject": "hello", "body": "hello" })),
            _ => complete("emailed"),
        }
    });
    let m = start_checked(script(move |c| {
        let agent = agent.clone();
        async move {
            if c.offered_tools().contains(&"review".to_string()) && c.text().contains("The send to judge") {
                return tool("review", json!({ "verdict": "flag", "reason": "mallory was never named; the person should confirm" }));
            }
            if c.offered_tools().contains(&"review".to_string()) {
                return tool("review", json!({ "criteria": [{ "id": "c1", "holds": true, "evidence": "the approved mail was sent" }] }));
            }
            agent(c).await
        }
    }))
    .await;
    let s = spec().app("mail", &mail, "public", "private", &[("list_sent", "read"), ("send", "reversible_write")]);
    let app = Appliance::start(s, &m).await;
    assert!(app.api("alice").put_policy("org", &org_policy(&permit_execute("mail__send"))).await.is_success());
    let dana = app.api("dana");
    let (t, run) = trial(&dana, "rev", "flag", "Email bob hello.", json!({ "reviewer": true })).await;

    let card = dana.wait_item(&t, "approval").await;
    assert_eq!(card["details"]["rule"], "reviewer:flag", "the flag is not marked as the reviewer's: {card}");
    assert!(card["details"]["why"].as_str().unwrap_or_default().contains("mallory"), "the reviewer's reason is not on the card: {card}");
    assert!(mail.calls_to("send").is_empty(), "the flagged send left before the person approved");
    assert!(dana.respond(card["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "once" })).await.is_success());
    let done = dana.wait_settled(&t).await;
    assert_eq!(done["status"], "completed", "{done}");
    assert_eq!(mail.calls_to("send").len(), 1, "the approved send did not go");
    assert!(dana.open_items(&t, "approval").await.is_empty(), "the reviewer flagged the approved send again");
    assert_eq!(reviewer_replies(&dana, &run).await, 2, "the send and the completion review, not the retry");
}

/// A send the kernel asks about still asks: the reviewer does not run on
/// it, and its pass could not have let it through.
#[tokio::test(flavor = "multi_thread")]
async fn a_send_the_kernel_asks_about_still_asks() {
    let mail = mcp::mail().start().await;
    let m = start_checked(script(|c| async move {
        if c.offered_tools().contains(&"review".to_string()) && c.text().contains("The send to judge") {
            return tool("review", json!({ "verdict": "pass", "reason": "looks fine" }));
        }
        if c.offered_tools().contains(&"review".to_string()) {
            return tool("review", json!({ "criteria": [{ "id": "c1", "holds": true, "evidence": "the approved mail was sent" }] }));
        }
        match c.turn() {
            0 => contract_unchecked("email bob", "bob was emailed"),
            1 => tool("mail__list_inbox", json!({})),
            2 => tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hello", "body": "hello" })),
            _ => complete("emailed"),
        }
    }))
    .await;
    let s = spec().app("mail", &mail, "private", "private+untrusted", &[("list_inbox", "read"), ("list_sent", "read"), ("send", "reversible_write")]);
    let app = Appliance::start(s, &m).await;
    assert!(app.api("alice").put_policy("org", &org_policy(&permit_execute("mail__send"))).await.is_success());
    let dana = app.api("dana");
    let (t, run) = trial(&dana, "rev", "ask", "Email bob hello.", json!({ "reviewer": true })).await;

    let card = dana.wait_item(&t, "approval").await;
    assert_eq!((card["details"]["rule"].clone(), card["details"]["tool"].clone()), (json!("kernel:rule_of_two"), json!("mail__send")), "{card}");
    assert_eq!(reviewer_replies(&dana, &run).await, 0, "the reviewer ran on a send the kernel asks about");
    assert!(dana.respond(card["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "once" })).await.is_success());
    assert_eq!(mail.effects("send"), 0, "the send left before approval");
    let t = dana.wait_settled(&t).await;
    assert_eq!(t["status"], "completed", "{t}");
    assert_eq!(mail.effects("send"), 1, "the approved send did not go");
}

/// Completion criteria no code check covers go to the reviewer before the
/// person: what it holds is recorded, what it flags stays for the person.
#[tokio::test(flavor = "multi_thread")]
async fn unchecked_criteria_go_to_the_reviewer_before_the_person() {
    let mail = mcp::mail().start().await;
    let m = start_checked(script(|c| async move {
        if c.offered_tools().contains(&"review".to_string()) {
            let holds = c.text().contains("Count the sent mail");
            return tool(
                "review",
                json!({ "criteria": [{ "id": "c1", "holds": holds, "evidence": if holds { "list_sent shows one message" } else { "no check shows the summary is right" } }] }),
            );
        }
        match c.turn() {
            0 => contract_unchecked("the mail was handled", "the mail was handled"),
            1 => tool("mail__list_sent", json!({})),
            _ => complete("handled"),
        }
    }))
    .await;
    let s = spec().app("mail", &mail, "public", "private", &[("list_sent", "read")]);
    let app = Appliance::start(s, &m).await;
    let dana = app.api("dana");

    let (holds, holds_run) = trial(&dana, "rev", "holds", "Count the sent mail.", json!({ "reviewer": true })).await;
    let done = dana.wait_settled(&holds).await;
    assert_eq!(done["status"], "completed", "a held criterion still asked the person: {done}");
    let (_, log) = dana.get(&format!("/v1/dev/runs/{holds_run}/log")).await;
    let verdicts: Vec<bool> =
        log["events"].as_array().unwrap().iter().filter(|e| e["kind"] == "verification" && e["body"]["by"] == "reviewer").map(|e| e["body"]["passed"].as_bool().unwrap()).collect();
    assert_eq!(verdicts, vec![true], "the reviewer did not judge the criterion");

    let (flags, _) = trial(&dana, "rev", "flags", "Summarize the mail.", json!({ "reviewer": true })).await;
    let t = dana.wait_settled(&flags).await;
    assert_eq!(t["waiting_on"], "review", "a flagged criterion did not stay for the person: {t}");
    let card = dana.wait_item(&flags, "review").await;
    assert!(card.to_string().contains("reviewer"), "the review card does not carry the reviewer's reason: {card}");
}

/// A once approval covers one flagged send: the bypass consumes it, so a
/// repeated identical send is judged again instead of riding along (#385).
#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_send_after_a_once_approval_is_judged_again() {
    let mail = mcp::mail().start().await;
    let agent = script(|c| async move {
        match c.turn() {
            0 => contract_unchecked("email bob", "bob was emailed"),
            1 | 2 => tool("mail__send", json!({ "to": "mallory@evil.example", "subject": "hello", "body": "hello" })),
            _ => complete("emailed"),
        }
    });
    let m = start_checked(script(move |c| {
        let agent = agent.clone();
        async move {
            if c.offered_tools().contains(&"review".to_string()) && c.text().contains("The send to judge") {
                return tool("review", json!({ "verdict": "flag", "reason": "mallory was never named; the person should confirm" }));
            }
            if c.offered_tools().contains(&"review".to_string()) {
                return tool("review", json!({ "criteria": [{ "id": "c1", "holds": true, "evidence": "the approved mail was sent" }] }));
            }
            agent(c).await
        }
    }))
    .await;
    let s = spec().app("mail", &mail, "public", "private", &[("list_sent", "read"), ("send", "reversible_write")]);
    let app = Appliance::start(s, &m).await;
    assert!(app.api("alice").put_policy("org", &org_policy(&permit_execute("mail__send"))).await.is_success());
    let dana = app.api("dana");
    let (t, run) = trial(&dana, "rev", "once", "Email bob hello.", json!({ "reviewer": true })).await;

    let first = dana.wait_item(&t, "approval").await;
    assert_eq!(first["details"]["rule"], "reviewer:flag", "{first}");
    assert!(dana.respond(first["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "once" })).await.is_success());
    // The approved send goes; the identical repeat is flagged again.
    let second = dana.wait_item(&t, "approval").await;
    assert_eq!(second["details"]["rule"], "reviewer:flag", "the repeat rode the consumed approval: {second}");
    assert_ne!(second["id"], first["id"], "the same card came back");
    assert_eq!(mail.calls_to("send").len(), 1, "the repeat sent without a judgment");
    assert_eq!(reviewer_replies(&dana, &run).await, 2, "both sends were not judged");
}

/// The completion reviewer judges with the run's tool results in front
/// of it, not the summary alone: an overstated report, with no send in
/// the results, is flagged for the person (#384).
#[tokio::test(flavor = "multi_thread")]
async fn the_completion_reviewer_sees_the_runs_tool_results() {
    let mail = mcp::mail().start().await;
    let m = start_checked(script(|c| async move {
        if c.offered_tools().contains(&"review".to_string()) {
            // Hold only when the run's results show the claimed send.
            let sent = c.text().contains("mail__send") && c.text().contains("\"status\": \"done\"");
            return tool(
                "review",
                json!({ "criteria": [{ "id": "c1", "holds": sent, "evidence": if sent { "the results show the send" } else { "no send in the run's results" } }] }),
            );
        }
        match c.turn() {
            0 => contract_unchecked("email bob", "bob was emailed"),
            1 => tool("mail__list_sent", json!({})),
            _ => complete("emailed bob"),
        }
    }))
    .await;
    let s = spec().app("mail", &mail, "public", "private", &[("list_sent", "read")]);
    let app = Appliance::start(s, &m).await;
    let dana = app.api("dana");
    let (t, _) = trial(&dana, "rev", "evidence", "Email bob hello.", json!({ "reviewer": true })).await;

    let done = dana.wait_settled(&t).await;
    assert_eq!(done["waiting_on"], "review", "the overstated report did not stay for the person: {done}");
    let reviews: Vec<_> = m.requests().into_iter().filter(|c| c.offered_tools().contains(&"review".to_string())).collect();
    assert!(!reviews.is_empty(), "the reviewer was never asked");
    assert!(reviews.iter().any(|c| c.text().contains("mail__list_sent")), "the reviewer never saw the run's tool results");
}
