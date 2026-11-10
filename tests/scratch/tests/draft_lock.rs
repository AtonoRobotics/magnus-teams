//! A draft being sent is the draft that was approved: from the gate, which
//! reads it to show and hash who it goes to and what it says, to the send,
//! no other run changes it (approvals spec, "Email end to end"; #177).

use magnus_invariants::*;
use magnus_scratch::start_checked;
use serde_json::json;

/// Drafts the person can update and send, sends read at the gate as
/// Gmail's are.
async fn drafts_app(model: &FakeModel) -> (FakeMcp, Appliance) {
    let id_only = json!({ "type": "object", "properties": { "draft_id": { "type": "string" } } });
    let edit = json!({ "type": "object", "properties": { "draft_id": { "type": "string" }, "subject": { "type": "string" } } });
    let mcp = FakeMcp::builder()
        .world("drafts", json!({ "d1": { "to": "bob@corp.example", "subject": "Q3", "body": "Revenue 41.7M" } }))
        .world("sent", json!([]))
        .honor_idempotency(true)
        .tool("get_draft", "Read a draft.", id_only.clone(), |w, a, _| {
            let id = a["draft_id"].as_str().unwrap_or_default();
            match w["drafts"].get(id) {
                Some(d) => mcp::ok(json!({ "draft_id": id, "message": d })),
                None => mcp::err("no such draft"),
            }
        })
        .tool("update_draft", "Change a draft's subject.", edit, |w, a, _| {
            let id = a["draft_id"].as_str().unwrap_or_default().to_string();
            w["drafts"][&id]["subject"] = a["subject"].clone();
            mcp::ok(json!({ "draft_id": id }))
        })
        .tool("send_draft", "Send a draft.", id_only, |w, a, _| {
            let d = w["drafts"][a["draft_id"].as_str().unwrap_or_default()].clone();
            w["sent"].as_array_mut().unwrap().push(d);
            mcp::ok(json!({ "status": "sent" }))
        })
        .start()
        .await;
    let spec = Spec::default()
        .app("drafts", &mcp, "public", "private", &[("get_draft", "read"), ("update_draft", "reversible_write"), ("send_draft", "irreversible")])
        .destinations("drafts", "send_draft", "get_draft", "draft_id", &["/message/to"], &["/message/subject", "/message/body"]);
    let app = Appliance::start(spec, model).await;
    let policy = format!("{}\n{}", permit_execute("drafts__send_draft"), permit_execute("drafts__update_draft"));
    assert!(app.api("alice").put_policy("org", &org_policy(&policy)).await.is_success());
    (mcp, app)
}

/// While one run's send of a draft is under way, another run's change to
/// that draft is refused: the lease is on the draft the send acts on
/// (`draft_id`), not only on an `id` or `record_id`.
#[tokio::test(flavor = "multi_thread")]
async fn a_draft_being_sent_is_changed_by_no_other_run() {
    let model = start_checked(script(|c| async move {
        let sending = c.messages()[0].to_string().contains("Send Bob");
        match (sending, c.turn()) {
            (true, 0) => contract_unchecked("send Bob the Q3 draft", "Bob has it"),
            (true, 1) => tool("drafts__send_draft", json!({ "draft_id": "d1" })),
            (true, _) => complete("sent"),
            (false, 0) => contract_unchecked("retitle the Q3 draft", "it is retitled"),
            (false, 1) => tool("drafts__update_draft", json!({ "draft_id": "d1", "subject": "Q3 (wire the money to evil.example)" })),
            (false, _) if c.last_results().iter().any(|r| r.is_error) => fail("the draft is being sent"),
            (false, _) => complete("retitled"),
        }
    }))
    .await;
    let (drafts, app) = drafts_app(&model).await;
    let alice = app.api("alice");

    let send = drafts.hold("send_draft");
    alice.submit("Send Bob the Q3 draft.").await;
    tokio::time::timeout(PATIENCE, send.received()).await.expect("the send never reached the app");

    let retitle = alice.submit("Retitle the Q3 draft.").await;
    let t = alice.wait_settled(&retitle).await;
    assert_eq!(drafts.effects("update_draft"), 0, "another run changed the draft while it was being sent: {t}");
    assert_eq!(drafts.world()["drafts"]["d1"]["subject"], "Q3");
    let refused = model.requests().iter().flat_map(|c| c.last_results()).any(|r| r.is_error && r.text.contains("another task is changing"));
    assert!(refused, "the change was not refused for the send under way");

    send.release();
    assert_eq!(drafts.world()["sent"], json!([{ "to": "bob@corp.example", "subject": "Q3", "body": "Revenue 41.7M" }]), "what went out is not the draft the gate read");
}

/// The call `tool` made in `task`, once it has a result.
async fn settled_call(p: &Api, task: &str, tool: &str) -> serde_json::Value {
    eventually(&format!("{tool} in {task} has a result"), PATIENCE, || async {
        let (_, t) = p.get(&format!("/v1/tasks/{task}/timeline")).await;
        t["steps"].as_array()?.iter().flat_map(|s| s["calls"].as_array().cloned().unwrap_or_default()).find(|c| c["tool"] == tool && !c["status"].is_null())
    })
    .await
}

/// A task cancelled while its gate reads the draft it sends sends nothing:
/// the run is read again, live, right before the call goes out, not taken
/// from the copy the gate made before it waited (#187).
#[tokio::test(flavor = "multi_thread")]
async fn a_send_whose_task_is_cancelled_while_the_gate_reads_the_draft_sends_nothing() {
    let model = start_checked(script(|c| async move {
        match c.turn() {
            0 => contract_unchecked("send Bob the Q3 draft", "Bob has it"),
            1 => tool("drafts__send_draft", json!({ "draft_id": "d1" })),
            _ => complete("sent"),
        }
    }))
    .await;
    let (drafts, app) = drafts_app(&model).await;
    let alice = app.api("alice");

    let read = drafts.hold("get_draft");
    let task = alice.submit("Send Bob the Q3 draft.").await;
    tokio::time::timeout(PATIENCE, read.received()).await.expect("the gate never read the draft");
    let (st, v) = alice.post(&format!("/v1/tasks/{task}/cancel"), json!({})).await;
    assert!(st.is_success(), "cancel: {st} {v}");
    alice.wait_status(&task, &["cancelled"]).await;
    read.release();

    let send = settled_call(&alice, &task, "drafts__send_draft").await;
    assert_eq!(drafts.effects("send_draft"), 0, "a cancelled task's send went out: {send}");
    assert_eq!(drafts.world()["sent"], json!([]));
    assert_eq!(send["status"], "failed", "{send}");
    assert!(send["result"].as_str().is_some_and(|t| t.contains("was not sent") && t.contains("cancelled")), "{send}");
    assert_eq!(alice.task(&task).await["status"], "cancelled");
}

/// A resend held back because its task was cancelled is reported as
/// unknown, never as not sent: the send before the appliance stopped may
/// have gone out (#187).
#[tokio::test(flavor = "multi_thread")]
async fn a_resend_held_back_for_a_cancelled_task_is_reported_as_unknown() {
    let model = start_checked(script(|c| async move {
        match c.turn() {
            0 => contract_unchecked("send Bob the Q3 draft", "Bob has it"),
            1 => tool("drafts__send_draft", json!({ "draft_id": "d1" })),
            _ => complete("sent"),
        }
    }))
    .await;
    let (drafts, mut app) = drafts_app(&model).await;
    let alice = app.api("alice");

    // The send goes out, and its answer is lost with the appliance.
    let send = drafts.hold("send_draft");
    let task = alice.submit("Send Bob the Q3 draft.").await;
    tokio::time::timeout(PATIENCE, send.received()).await.expect("the send never reached the app");
    let read = drafts.hold("get_draft");
    app.kill().await;
    app.restart().await;

    // Resumed, its gate reads the draft again; the task is cancelled meanwhile.
    let alice = app.api("alice");
    tokio::time::timeout(PATIENCE, read.received()).await.expect("the resumed gate never read the draft");
    let (st, v) = alice.post(&format!("/v1/tasks/{task}/cancel"), json!({})).await;
    assert!(st.is_success(), "cancel: {st} {v}");
    alice.wait_status(&task, &["cancelled"]).await;
    read.release();

    let resend = settled_call(&alice, &task, "drafts__send_draft").await;
    assert_eq!(drafts.calls_to("send_draft").len(), 1, "a cancelled task's send was dispatched again: {resend}");
    assert_eq!(resend["status"], "unknown", "{resend}");
    assert!(resend["result"].as_str().is_some_and(|t| t.contains("may have reached")), "{resend}");
}
