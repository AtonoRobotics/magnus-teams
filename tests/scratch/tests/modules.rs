//! Removable modules and their ablation suites: each one compensates for a
//! model weakness, is switched per model and pinned with each run (K9), and
//! is compared on and off as an experiment.
//! - lost-reset: a run going in circles is cut back with a note;
//! - escalation: lost again, it moves to the stronger model;
//! - verifier: judges the criteria no code checks, by reading the world;
//! - reviewer: judges every send before it leaves, and the criteria no code
//!   checks, blocking or flagging what should not go.

use magnus_invariants::appliance::Person;
use magnus_invariants::model::{Reply, Script};
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::{json, Value};

const NOTE: &str = "You were going in circles";

fn spec() -> Spec {
    let mut s = Spec::default();
    s.people.push(Person { id: "dana".into(), name: "Dana".into(), token: "tok-dana-modules".into(), groups: vec!["staff".into()], roles: vec!["developer".into()] });
    s.max_model_calls = 12;
    s
}

/// The kinds of a run's events, in order.
async fn kinds(dana: &Api, run: &str) -> Vec<String> {
    let (_, log) = dana.get(&format!("/v1/dev/runs/{run}/log")).await;
    log["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap_or_default().to_string()).collect()
}

async fn trial(dana: &Api, exp: &str, case: &str, text: &str, modules: Value) -> (String, String) {
    let (st, v) = dana.post(&format!("/v1/dev/experiments/{exp}/trials"), json!({ "case": case, "trial": 0, "text": text, "modules": modules })).await;
    assert_eq!(st, 201, "{v}");
    (v["task_id"].as_str().unwrap().to_string(), v["run_id"].as_str().unwrap().to_string())
}

/// Checks the sent mail over and over; told it is going in circles, it
/// stops and reports.
fn looper() -> Script {
    script(|c| async move {
        if c.turn() == 0 {
            return contract_unchecked("check the sent mail", "the sent mail was checked");
        }
        if c.text().contains(NOTE) {
            return complete("checked");
        }
        tool("mail__list_sent", json!({}))
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_looping_run_is_reset_and_recovers_and_without_the_module_runs_to_its_budget() {
    let mail = mcp::mail().start().await;
    let m = start_checked(looper()).await;
    let app = Appliance::start(spec().app("mail", &mail, "public", "private", &[("list_sent", "read")]), &m).await;
    let dana = app.api("dana");

    let (on, on_run) = trial(&dana, "lost", "loop", "Check the sent mail.", json!({})).await;
    let (off, off_run) = trial(&dana, "lost", "loop", "Check the sent mail.", json!({ "lost_reset": false, "escalation": false })).await;

    // On: cut back after the same call returned the same result 4 times; the
    // model sees the note instead of the loop, and finishes.
    let t = dana.wait_settled(&on).await;
    assert_eq!(t["status"], "completed", "{t}");
    let k = kinds(&dana, &on_run).await;
    assert_eq!(k.iter().filter(|k| *k == "context_reset").count(), 1, "{k:?}");
    let after = m.requests().into_iter().find(|c| c.text().contains(NOTE)).expect("the model never saw the note");
    assert_eq!(after.turn(), 1, "the loop was not cut out of the context");
    assert!(after.text().contains("mail__list_sent ×4"), "the note does not say what the cut replies did");

    // Off: the same model loops until the budget stops it.
    let t = dana.wait_settled(&off).await;
    assert_eq!(t["waiting_on"], "budget", "{t}");
    assert!(!kinds(&dana, &off_run).await.contains(&"context_reset".to_string()));

    // The ablation: one experiment, two configurations, told apart by the module.
    let (_, e) = dana.get("/v1/dev/experiments/lost?k=1").await;
    let keys: Vec<String> = e["configs"].as_array().unwrap().iter().map(|c| c["key"].as_str().unwrap_or_default().to_string()).collect();
    assert_eq!(keys.len(), 2, "{e}");
    assert!(keys.iter().any(|k| k.contains("lost_reset")) && keys.iter().any(|k| !k.contains("lost_reset")), "{keys:?}");
    let (st, _) = dana.post("/v1/dev/experiments/lost/trials", json!({ "case": "loop", "trial": 0, "text": "x", "modules": { "lost_reset": false, "escalation": false } })).await;
    assert_eq!(st, 409, "the same trial ran twice with the same modules");
    let (st, _) = dana.post("/v1/dev/experiments/lost/trials", json!({ "case": "loop", "trial": 1, "text": "x", "modules": { "memory": false } })).await;
    assert_eq!(st, 400, "an unknown module was accepted");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_lost_again_escalates_to_the_stronger_model() {
    let mail = mcp::mail().start().await;
    // Only the stronger model gets out of the loop.
    let m = start_checked(script(|c| async move {
        if c.turn() == 0 {
            return contract_unchecked("check the sent mail", "the sent mail was checked");
        }
        if c.body["model"] == "claude-strong" {
            return complete("checked");
        }
        tool("mail__list_sent", json!({}))
    }))
    .await;
    let mut app = Appliance::start(spec().app("mail", &mail, "public", "private", &[("list_sent", "read")]), &m).await;
    reconfigure(&mut app, |cfg| {
        let per: toml::Value = toml::from_str(r#"model = [{ model = "anthropic/claude-test", escalate_to = "anthropic/claude-strong" }]"#).unwrap();
        cfg.insert("modules".into(), per);
    })
    .await;
    let dana = app.api("dana");
    let (on, on_run) = trial(&dana, "esc", "loop", "Check the sent mail.", json!({})).await;
    let (off, _) = trial(&dana, "esc", "loop", "Check the sent mail.", json!({ "escalation": false })).await;

    // Reset once, lost again: escalated, with a fresh context, and the
    // stronger model finishes.
    let t = dana.wait_settled(&on).await;
    assert_eq!(t["status"], "completed", "{t}");
    let k = kinds(&dana, &on_run).await;
    assert_eq!((k.iter().filter(|k| *k == "context_reset").count(), k.iter().filter(|k| *k == "escalated").count()), (2, 1), "{k:?}");
    let strong: Vec<_> = m.requests().into_iter().filter(|c| c.body["model"] == "claude-strong").collect();
    assert!(!strong.is_empty() && strong[0].text().contains(NOTE), "the stronger model did not start from the note");
    let (_, r) = dana.get(&format!("/v1/dev/runs/{on_run}")).await;
    assert_eq!(r["run"]["escalated_to"], "anthropic/claude-strong", "{}", r["run"]);
    assert_eq!(r["run"]["config"]["model"], "claude-test", "the pinned configuration changed");

    // Without escalation, lost again means a person decides.
    let t = dana.wait_settled(&off).await;
    assert_eq!(t["waiting_on"], "lost", "{t}");

    // The registry: each module, what it compensates for, its ablation
    // suite, and where it is on.
    let (_, reg) = dana.get("/v1/dev/modules").await;
    let esc = reg["modules"].as_array().unwrap().iter().find(|m| m["name"] == "escalation").unwrap().clone();
    assert_eq!(esc["models"]["anthropic/claude-test"], json!({ "on": true, "escalate_to": "anthropic/claude-strong" }), "{reg}");
    assert!(esc["compensates"].as_str().is_some_and(|c| !c.is_empty()) && esc["ablation_suite"].as_str().unwrap().contains("modules.rs"));
    assert_eq!(esc["acted"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_verifier_catches_work_its_own_report_overstates() {
    let mail = mcp::mail().start().await;
    let m = start_checked(script(|c| async move {
        let t = c.text();
        // The verifier, before work: it reviews the criteria.
        if c.offered_tools().is_empty() && t.contains("The agent's criteria") {
            return Reply::Text("The request names a subject, hello; no criterion covers it.".into());
        }
        // The verifier, after: it reads the sent mail and judges.
        if c.offered_tools().contains(&"verdict".to_string()) {
            return match c.last_results().first() {
                None => tool("mail__list_sent", json!({})),
                Some(r) => {
                    let holds = r.text.contains("bob@corp.example");
                    tool(
                        "verdict",
                        json!({ "criteria": [{ "id": "c1", "holds": holds, "evidence": if holds { "a message to bob is in sent" } else { "sent has no message to bob" } }] }),
                    )
                }
            };
        }
        // The agent: says it sent the mail before it has; told otherwise, sends it.
        match c.turn() {
            0 => contract_unchecked("email bob hello", "bob was sent an email"),
            _ if t.contains("The verifier found") && !json!(c.messages()).to_string().contains("\"mail__send\"") => {
                tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hello", "body": "hello" }))
            }
            _ => complete("sent"),
        }
    }))
    .await;
    let mut s = spec().app("mail", &mail, "public", "private", &[("list_sent", "read"), ("send", "reversible_write")]);
    s.verifier = true;
    let app = Appliance::start(s, &m).await;
    assert!(app.api("alice").put_policy("org", &org_policy(&permit_execute("mail__send"))).await.is_success());
    let dana = app.api("dana");
    let (on, on_run) = trial(&dana, "ver", "send", "Email bob hello.", json!({})).await;

    let t = dana.wait_settled(&on).await;
    assert_eq!(t["status"], "completed", "the verifier's verdict did not complete it: {t}");
    assert_eq!(mail.effects("send"), 1);
    let agent = m.requests().into_iter().filter(|c| c.offered_tools().contains(&"task_complete".to_string())).last().unwrap();
    assert!(agent.text().contains("The verifier's review of your criteria") && agent.text().contains("subject"), "the criteria review did not reach the agent");
    let (_, log) = dana.get(&format!("/v1/dev/runs/{on_run}/log")).await;
    let verdicts: Vec<bool> =
        log["events"].as_array().unwrap().iter().filter(|e| e["kind"] == "verification" && e["body"]["by"] == "verifier").map(|e| e["body"]["passed"].as_bool().unwrap()).collect();
    assert_eq!(verdicts, vec![false, true], "the verifier judged the report, not the world");
    let k = kinds(&dana, &on_run).await;
    assert!(k.iter().filter(|k| *k == "verifier_reply").count() >= 3, "{k:?}");
    // Its reads went through the boundary.
    assert!(mail.calls_to("list_sent").len() >= 2, "the verifier did not read the world");

    // Off: the same overstated report completes, unconfirmed.
    let (off, _) = trial(&dana, "ver", "send", "Email bob hello.", json!({ "verifier": false })).await;
    let t = dana.wait_settled(&off).await;
    assert_eq!(t["status"], "completed", "{t}");
}

/// The reviewer's ablation: on, a send to the wrong recipient never leaves;
/// off, the same send goes. The registry names this suite for the module.
#[tokio::test(flavor = "multi_thread")]
async fn the_reviewer_blocks_a_send_to_the_wrong_recipient() {
    let mail = mcp::mail().start().await;
    let m = start_checked(script(|c| async move {
        // The reviewer blocks mail to mallory, and holds the completion.
        if c.offered_tools().contains(&"review".to_string()) {
            if !c.text().contains("The send to judge") {
                return tool("review", json!({ "criteria": [{ "id": "c1", "holds": true, "evidence": "the report matches the run" }] }));
            }
            let bad = c.text().contains("mallory@evil.example");
            return tool(
                "review",
                json!({ "verdict": if bad { "block" } else { "pass" }, "reason": if bad { "mallory is not the intended recipient" } else { "to the named recipient" } }),
            );
        }
        // The agent mails mallory, of all people.
        match c.turn() {
            0 => contract_unchecked("email mallory", "mallory was emailed"),
            1 => tool("mail__send", json!({ "to": "mallory@evil.example", "subject": "hello", "body": "hello" })),
            _ if c.text().contains("Denied (reviewer)") => complete("blocked; nothing sent"),
            _ => complete("sent"),
        }
    }))
    .await;
    let s = spec().app("mail", &mail, "public", "private", &[("list_sent", "read"), ("send", "reversible_write")]);
    let app = Appliance::start(s, &m).await;
    assert!(app.api("alice").put_policy("org", &org_policy(&permit_execute("mail__send"))).await.is_success());
    let dana = app.api("dana");

    // On: the send is blocked, and the run finishes without sending.
    let (on, on_run) = trial(&dana, "rev", "send", "Email mallory hello.", json!({ "reviewer": true })).await;
    let t = dana.wait_settled(&on).await;
    assert_eq!(t["status"], "completed", "{t}");
    assert_eq!(mail.effects("send"), 0, "the blocked send left");
    let k = kinds(&dana, &on_run).await;
    assert!(k.iter().any(|k| k == "reviewer_reply"), "{k:?}");

    // Off: the same send goes.
    let (off, _) = trial(&dana, "rev", "send", "Email mallory hello.", json!({})).await;
    let t = dana.wait_settled(&off).await;
    assert!(t["status"] == "completed" || t["waiting_on"] == "review", "{t}");
    assert_eq!(mail.effects("send"), 1, "the unreviewed send did not go");

    // The registry lists the module, its ablation suite, and its acts.
    let (_, reg) = dana.get("/v1/dev/modules").await;
    let rev = reg["modules"].as_array().unwrap().iter().find(|m| m["name"] == "reviewer").unwrap().clone();
    assert!(rev["compensates"].as_str().is_some_and(|c| !c.is_empty()) && rev["ablation_suite"].as_str().unwrap().contains("modules.rs"));
    assert!(rev["models"].as_object().unwrap().values().all(|m| m["on"] == json!(false)), "the reviewer is on by default: {reg}");
    assert!(rev["acted"].as_u64().unwrap() >= 2, "{reg}");
}
