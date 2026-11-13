//! Invariant 7: results are checked against the world, not against the
//! agent's report. Evidence comes from the record (spec rev 147): no
//! contract or criteria come before the work, and after each irreversible
//! action the server reads the world back and attaches what it found.
//!
//! Prevents: agents declaring completion of work the world does not show.

// Test code: a failed unwrap is a failed test (AGENTS.md: none outside tests).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use magnus_invariants::{mcp, *};
use serde_json::{json, Value};
use std::time::Duration;

/// A record store whose `set_status` is irreversible and read back with
/// `get_record`; its `claim_status` says it set the status and does not.
fn crm_spec(crm: &FakeMcp) -> Spec {
    Spec::default()
        .app("crm", crm, "internal", "private", &[("get_record", "read"), ("set_status", "irreversible"), ("claim_status", "irreversible")])
        .read_back("crm", "set_status", "get_record", &[("id", "/input/id")], &[("/input/status", "/status")])
        .read_back("crm", "claim_status", "get_record", &[("id", "/input/id")], &[("/input/status", "/status")])
}

fn crm() -> mcp::Builder {
    let schema = json!({ "type": "object", "properties": { "id": { "type": "string" }, "status": { "type": "string" } } });
    mcp::crm()
        .tool("set_status", "Set a record's status.", schema.clone(), |w, a, _| {
            let id = a["id"].as_str().unwrap_or_default().to_string();
            let Some(rec) = w["records"].get_mut(&id) else { return mcp::err("no such record") };
            rec["status"] = a["status"].clone();
            mcp::ok(rec.clone())
        })
        // Says it is done; changes nothing.
        .tool("claim_status", "Set a record's status.", schema, |_, a, _| mcp::ok(json!({ "id": a["id"], "status": a["status"] })))
}

async fn started(crm: &FakeMcp, model: &FakeModel) -> Appliance {
    let app = Appliance::start(crm_spec(crm), model).await;
    let policy = org_policy(&format!("{}\n{}", permit_execute("crm__set_status"), permit_execute("crm__claim_status")));
    assert!(app.api("alice").put_policy("org", &policy).await.is_success());
    app
}

fn receipts(t: &Value) -> Vec<Value> {
    t["evidence"]["receipts"].as_array().cloned().unwrap_or_default()
}

/// The agent says it closed the record; the app said so too; the world
/// does not show it. The read-back catches it: the agent is told, and the
/// result shows the action as not done, whatever the report says.
#[tokio::test(flavor = "multi_thread")]
async fn a_false_report_of_completion_is_caught_by_reading_the_world_back() {
    let crm = crm().start().await;
    let model = FakeModel::start(steps(vec![tool("crm__claim_status", json!({ "id": "42", "status": "closed" })), complete("closed it"), fail("unexpected turn")])).await;
    let app = started(&crm, &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Close CRM record 42.").await;
    let t = alice.wait_settled(&id).await;

    let told = &model.at_turn(1).expect("the run went on after the action").last_results()[0];
    assert!(told.text.contains("does not match"), "the agent was not told the world differs: {}", told.text);
    let r = receipts(&t);
    assert_eq!(r.len(), 1, "{t}");
    assert_eq!(r[0]["read_back"]["matches"], false, "the read-back did not catch it: {t}");
    let not_done = t["evidence"]["not_done"].as_array().cloned().unwrap_or_default();
    assert!(not_done.iter().any(|n| n["status"] == "read_back_differs"), "the result shows the claim as done: {t}");
    assert_eq!(crm.world()["records"]["42"]["status"], "open");
}

/// An action that did happen is read back and matches: the receipt comes
/// from the record with what the server found, not from the agent's word.
#[tokio::test(flavor = "multi_thread")]
async fn what_was_done_is_read_back_from_the_world() {
    let crm = crm().start().await;
    let model = FakeModel::start(steps(vec![tool("crm__set_status", json!({ "id": "42", "status": "closed" })), complete("closed it"), fail("unexpected turn")])).await;
    let app = started(&crm, &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Close CRM record 42.").await;
    let t = alice.wait_settled(&id).await;

    assert_eq!(t["status"], "completed", "{t}");
    let r = receipts(&t);
    assert_eq!(r.len(), 1, "{t}");
    assert_eq!(r[0]["read_back"]["matches"], true, "{t}");
    assert_eq!(r[0]["read_back"]["found"]["status"], "closed", "{t}");
    assert!(t["evidence"]["not_done"].as_array().is_some_and(|n| n.is_empty()), "{t}");
}

/// A read-back only reads: reading back never changes the world, and a
/// read-back set up with a tool that writes is refused before the server
/// starts.
#[tokio::test(flavor = "multi_thread")]
async fn read_backs_may_only_read() {
    let crm = crm().start().await;
    let model = FakeModel::start(steps(vec![tool("crm__set_status", json!({ "id": "42", "status": "closed" })), complete("closed it"), fail("unexpected turn")])).await;
    let app = started(&crm, &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Close CRM record 42.").await;
    alice.wait_settled(&id).await;
    assert_eq!(crm.effects("set_status"), 1, "reading back changed the world");
    assert_eq!(crm.calls_to("get_record").len(), 1, "the action was not read back, or read back more than once");
    assert_eq!(crm.calls().len(), 2, "the read-back called something other than its read");
    drop(app);

    // A read-back that would write: the server refuses the configuration.
    let writer = crm_spec(&crm).read_back("crm", "set_status", "claim_status", &[("id", "/input/id")], &[]);
    let model = FakeModel::start(steps(vec![fail("unexpected turn")])).await;
    let started =
        tokio::time::timeout(Duration::from_secs(60), tokio::spawn(async move { Appliance::start(writer, &model).await })).await.expect("the server neither started nor refused");
    assert!(started.is_err(), "the server started with a read-back that writes");
}

/// A mailbox that keeps what it sends: `send` returns the message's id, and
/// `get_message` reads it back from the sent folder.
fn mailbox() -> mcp::Builder {
    let send = json!({ "type": "object", "properties": { "to": { "type": "string" }, "subject": { "type": "string" }, "body": { "type": "string" } } });
    mcp::FakeMcp::builder()
        .world("sent", json!([]))
        .honor_idempotency(true)
        .tool("send", "Send an email.", send, |w, a, _| {
            let sent = w["sent"].as_array_mut().unwrap();
            let id = format!("m{}", sent.len() + 1);
            sent.push(json!({ "id": id, "to": a["to"], "subject": a["subject"], "body": a["body"] }));
            mcp::ok(json!({ "message_id": id }))
        })
        .tool("get_message", "Read a sent message by id.", json!({ "type": "object", "properties": { "message_id": { "type": "string" } } }), |w, a, _| {
            let id = a["message_id"].as_str().unwrap_or_default();
            match w["sent"].as_array().unwrap().iter().find(|m| m["id"] == id) {
                Some(m) => mcp::ok(m.clone()),
                None => mcp::err("no such message"),
            }
        })
}

/// Acceptance (spec rev 147): a task that sends an email ends with a
/// receipt and a read-back, and no contract was written. The agent sends
/// and reports; the server's record says what went where, and the sent
/// folder, read back, agrees.
#[tokio::test(flavor = "multi_thread")]
async fn a_sent_email_ends_with_a_receipt_and_a_read_back_and_no_contract() {
    let mail = mailbox().start().await;
    let send = tool("mail__send", json!({ "to": "bob@corp.example", "subject": "Q3 numbers", "body": "Attached are the Q3 numbers." }));
    let model = FakeModel::start(steps(vec![send, complete("Sent Bob the Q3 numbers."), fail("unexpected turn")])).await;
    let spec = Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible"), ("get_message", "read")]).read_back(
        "mail",
        "send",
        "get_message",
        &[("message_id", "/result/message_id")],
        &[("/input/to", "/to"), ("/input/subject", "/subject")],
    );
    let app = Appliance::start(spec, &model).await;
    let alice = app.api("alice");
    assert!(alice.put_policy("org", &org_policy(&permit_execute("mail__send"))).await.is_success());
    let id = alice.submit("Email Bob the Q3 numbers.").await;
    let t = alice.wait_settled(&id).await;

    assert_eq!(t["status"], "completed", "{t}");
    assert_eq!(mail.effects("send"), 1);
    let r = receipts(&t);
    assert_eq!(r.len(), 1, "{t}");
    assert_eq!(r[0]["tool"], "mail__send", "{t}");
    assert_eq!(r[0]["to"], json!(["bob@corp.example"]), "{t}");
    assert_eq!(r[0]["read_back"]["matches"], true, "{t}");
    assert_eq!(r[0]["read_back"]["found"]["subject"], "Q3 numbers", "{t}");
    assert!(t["evidence"]["not_done"].as_array().is_some_and(|n| n.is_empty()), "{t}");
    assert!(model.requests().iter().all(|c| !c.offered_tools().iter().any(|n| n == "task_contract")), "a contract tool was offered");
    let log = app.db.dump().await.replace(' ', "");
    assert!(!log.contains("\"kind\":\"contract\"") && !log.contains("task_contract"), "a contract was written");
}
