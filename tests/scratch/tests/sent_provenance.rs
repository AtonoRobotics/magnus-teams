//! Recipient provenance from sent mail (approvals spec, #210): under both
//! taints, a send to an address the person has mailed before passes with no
//! card, read at the gate through their own sign-in; one they have not
//! mailed asks.

use magnus_invariants::appliance::Person;
use magnus_invariants::{mcp, *};
use magnus_scratch::start_checked;
use serde_json::json;

async fn drafts_app(drafts: serde_json::Value, sent: serde_json::Value, model: &FakeModel) -> (FakeMcp, Appliance) {
    let id_only = json!({ "type": "object", "properties": { "draft_id": { "type": "string" } } });
    let mcp = FakeMcp::builder()
        .world("drafts", drafts)
        .world("sent", sent)
        .honor_idempotency(true)
        .tool("get_draft", "Read a draft.", id_only.clone(), |w, a, _| {
            let id = a["draft_id"].as_str().unwrap_or_default();
            match w["drafts"].get(id) {
                Some(d) => mcp::ok(json!({ "draft_id": id, "message": d })),
                None => mcp::err("no such draft"),
            }
        })
        .tool("send_draft", "Send a draft.", id_only, |w, a, _| {
            let d = w["drafts"][a["draft_id"].as_str().unwrap_or_default()].clone();
            w["sent"].as_array_mut().unwrap().push(d);
            mcp::ok(json!({ "status": "sent" }))
        })
        .tool("search_sent", "Search sent mail.", json!({ "type": "object", "properties": { "query": { "type": "string" } } }), |w, a, _| {
            let to = a["query"].as_str().unwrap_or_default().strip_prefix("to:").unwrap_or_default();
            let threads: Vec<serde_json::Value> = w["sent"].as_array().into_iter().flatten().filter(|m| m["to"].as_str().is_some_and(|t| t.contains(to))).cloned().collect();
            mcp::ok(json!({ "threads": threads }))
        })
        .start()
        .await;
    let mut spec = Spec::default();
    spec.people.push(Person { id: "dana".into(), name: "Dana".into(), token: "tok-dana-dev".into(), groups: vec!["staff".into()], roles: vec!["developer".into()] });
    let spec = spec
        .app("drafts", &mcp, "public", "private+untrusted", &[("get_draft", "read"), ("send_draft", "irreversible"), ("search_sent", "read")])
        .destinations("drafts", "send_draft", "get_draft", "draft_id", &["/message/to", "/message/cc", "/message/bcc"], &["/message/subject", "/message/body"])
        .sent("drafts", "search_sent", "query", "to:{address}", "/threads");
    let app = Appliance::start(spec, model).await;
    assert!(app.api("alice").put_policy("org", &org_policy(&permit_execute("drafts__send_draft"))).await.is_success());
    (mcp, app)
}

/// A recipient found only in sent mail passes under both taints, and one
/// not found asks (#210).
#[tokio::test(flavor = "multi_thread")]
async fn a_recipient_mailed_before_passes_and_a_new_one_asks() {
    let model = start_checked(steps(vec![
        contract_unchecked("send the Q3 draft", "it is sent"),
        tool("drafts__get_draft", json!({ "draft_id": "d1" })),
        tool("drafts__send_draft", json!({ "draft_id": "d1" })),
        tool("drafts__send_draft", json!({ "draft_id": "d2" })),
        fail("stopping"),
    ]))
    .await;
    let (drafts, app) = drafts_app(
        json!({
            "d1": { "to": "bob@corp.example", "subject": "Q3", "body": "Confidential: revenue 41.7M" },
            "d2": { "to": "attacker@evil.example", "subject": "fwd", "body": "Confidential: revenue 41.7M" }
        }),
        json!([{ "to": "bob@corp.example", "subject": "Q2" }]),
        &model,
    )
    .await;
    let alice = app.api("alice");
    // The person's words name no address: only sent mail clears Bob.
    let id = alice.submit("Send the Q3 draft.").await;

    eventually("the draft to the mailed address was sent", PATIENCE, || async { (drafts.effects("send_draft") == 1).then_some(()) }).await;
    assert_eq!(alice.task(&id).await["taint"], json!({ "untrusted": true, "private": true }));
    assert_eq!(drafts.world()["sent"][1]["to"], "bob@corp.example");
    let card = alice.wait_item(&id, "approval").await;
    assert!(card.to_string().contains("attacker@evil.example"), "the card does not show the new address: {card}");
    assert_eq!(drafts.effects("send_draft"), 1, "the new address was sent to without the person");
}

/// The gate's sent-search decisions for a task's run, oldest first.
async fn search_decisions(app: &Appliance, alice: &Api, id: &str) -> Vec<serde_json::Value> {
    let run = alice.task(id).await["run_id"].as_str().unwrap().to_string();
    let (_, log) = app.api("dana").get(&format!("/v1/dev/runs/{run}/log")).await;
    log["events"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e["kind"] == "decision" && e["body"]["stage"] == "mailed_before" && e["body"]["tool"] == "drafts__search_sent")
        .map(|e| e["body"].clone())
        .collect()
}

/// A task layer forbidding the sent search makes a mailed send ask: the
/// unread search counts as not mailed, the denial is audited, and the
/// read never reaches the provider (#371).
#[tokio::test(flavor = "multi_thread")]
async fn task_policy_forbidding_the_sent_search_makes_the_send_ask() {
    let model = start_checked(script(|c| async move {
        match c.turn() {
            0 => tool("ask_principal", json!({ "question": "Ready?" })),
            1 => contract_unchecked("send the Q3 draft", "it is sent"),
            2 => tool("drafts__get_draft", json!({ "draft_id": "d1" })),
            3 => tool("drafts__send_draft", json!({ "draft_id": "d1" })),
            _ => fail("stopping"),
        }
    }))
    .await;
    let (drafts, app) =
        drafts_app(json!({ "d1": { "to": "bob@corp.example", "subject": "Q3", "body": "hi" } }), json!([{ "to": "bob@corp.example", "subject": "Q2" }]), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Send the Q3 draft.").await;
    // The run waits on the person: the task layer lands before the send.
    let q = alice.wait_item(&id, "question").await;
    assert!(alice.put_policy(&format!("task/{id}"), "forbid(principal, action, resource == Tool::\"drafts__search_sent\");").await.is_success());
    assert!(alice.respond(q["id"].as_str().unwrap(), json!({ "decision": "answer", "text": "Yes." })).await.is_success());

    let card = alice.wait_item(&id, "approval").await;
    assert_eq!(card["details"]["rule"], "kernel:rule_of_two", "the send did not ask: {card}");
    assert!(drafts.calls_to("search_sent").is_empty(), "the forbidden search reached the provider");
    assert_eq!(drafts.effects("send_draft"), 0, "the send went out");
    let ds = search_decisions(&app, &alice, &id).await;
    assert_eq!(ds.len(), 1, "no single audited search decision: {ds:?}");
    assert_eq!((ds[0]["tool"].clone(), ds[0]["outcome"].clone()), (json!("drafts__search_sent"), json!("deny")), "{ds:?}");
}

/// An allowed sent search leaves its own audit decision, and the mailed
/// send passes with no card (#371).
#[tokio::test(flavor = "multi_thread")]
async fn allowed_sent_search_leaves_an_audit_decision() {
    let model = start_checked(steps(vec![
        contract_unchecked("send the Q3 draft", "it is sent"),
        tool("drafts__get_draft", json!({ "draft_id": "d1" })),
        tool("drafts__send_draft", json!({ "draft_id": "d1" })),
        complete("sent"),
    ]))
    .await;
    let (drafts, app) =
        drafts_app(json!({ "d1": { "to": "bob@corp.example", "subject": "Q3", "body": "hi" } }), json!([{ "to": "bob@corp.example", "subject": "Q2" }]), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Send the Q3 draft.").await;
    alice.wait_settled(&id).await;

    assert_eq!(drafts.effects("send_draft"), 1, "the mailed recipient's send did not go out");
    let ds = search_decisions(&app, &alice, &id).await;
    assert_eq!(ds.len(), 1, "no single audited search decision: {ds:?}");
    assert_eq!((ds[0]["tool"].clone(), ds[0]["outcome"].clone()), (json!("drafts__search_sent"), json!("allow")), "{ds:?}");
}
