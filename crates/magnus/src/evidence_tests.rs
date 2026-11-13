use super::*;
use crate::log::Event;
use serde_json::json;

fn ev(seq: i64, actor: &str, ev: Value) -> Event {
    let Ok(ev) = serde_json::from_value(ev) else { panic!("not an event") };
    Event { seq, at: Utc::now() + chrono::Duration::milliseconds(seq), actor: actor.into(), ev }
}

fn class(t: &str) -> Option<(String, Risk)> {
    match t {
        "mail__send" => Some(("mail".into(), Risk::Irreversible)),
        "mail__get_sent" => Some(("mail".into(), Risk::Read)),
        "crm__update" => Some(("crm".into(), Risk::ReversibleWrite)),
        _ => None,
    }
}

fn started() -> State {
    let mut s = State::default();
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 9, "max_actions": 9, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    s.apply(&ev(1, "person:alice", json!({ "kind": "task_created", "task": "t1", "run": "r1", "person": "alice", "text": "Email bob the numbers.", "config": config })));
    s
}

fn call(s: &mut State, seq: i64, id: &str, tool: &str, input: Value, issuer: &str, status: &str, is_error: bool) {
    s.apply(&ev(seq, "kernel", json!({ "kind": "intent", "run": "r1", "call_id": id, "step_id": "s", "key": id, "tool": tool, "input": input, "issuer": issuer })));
    let label = json!({ "private": false, "untrusted": false });
    s.apply(&ev(seq + 1, "kernel", json!({ "kind": "tool_result", "run": "r1", "call_id": id, "key": id, "status": status, "text": "ok", "is_error": is_error, "label": label, "data": { "message_id": "m1" } })));
}

/// A receipt comes from the boundary log: the send went through, so it is
/// there with where it went and what the server read back after it; a read
/// is not a receipt, and nothing the agent says adds one.
#[test]
fn a_receipt_is_what_the_log_shows_went_through() {
    let mut s = started();
    call(&mut s, 2, "c1", "mail__get_sent", json!({}), "model", "done", false);
    call(&mut s, 4, "c2", "mail__send", json!({ "to": "bob@corp.example", "subject": "Q3" }), "model", "done", false);
    let rb = json!({ "kind": "evidence", "change": "read_back", "run": "r1", "call_id": "c2", "tool": "mail__send", "read": "get_sent", "found": { "to": "bob@corp.example" }, "matches": true, "note": "Read back with get_sent: it matches what was done." });
    s.apply(&ev(6, "kernel", rb));
    let run = s.runs["r1"].clone();
    let r = receipts(&s, &run, &class);
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(r[0]["call_id"], "c2");
    assert_eq!(r[0]["to"], json!(["bob@corp.example"]));
    assert_eq!(r[0]["read_back"]["matches"], true);
    assert!(not_done(&s, &run, &class).is_empty());
}

/// What was refused or failed is "not done", and so is a send whose
/// read-back did not match: the result never shows it as done.
#[test]
fn what_failed_or_did_not_read_back_is_not_done() {
    let mut s = started();
    call(&mut s, 2, "c1", "crm__update", json!({ "id": "1" }), "model", "failed", true);
    call(&mut s, 4, "c2", "mail__send", json!({ "to": "bob@corp.example" }), "model", "done", false);
    let rb = json!({ "kind": "evidence", "change": "read_back", "run": "r1", "call_id": "c2", "tool": "mail__send", "read": "get_sent", "found": {}, "matches": false, "note": "it does not match (to differs)" });
    s.apply(&ev(6, "kernel", rb));
    let run = s.runs["r1"].clone();
    let n = not_done(&s, &run, &class);
    assert_eq!(n.len(), 2, "{n:?}");
    assert_eq!(n[0]["status"], "failed");
    assert_eq!(n[1]["status"], "read_back_differs");
}

/// A source must name a call of this task that returned a result.
#[test]
fn a_source_names_a_call_that_returned() {
    let mut s = started();
    call(&mut s, 2, "c1", "mail__get_sent", json!({}), "model", "done", false);
    call(&mut s, 4, "c2", "mail__get_sent", json!({}), "model", "failed", true);
    let run = s.runs["r1"].clone();
    assert_eq!(cited(&run, &json!([{ "claim": "Revenue was $4.2M", "call": "c1" }])).map(|v| v.len()), Ok(1));
    assert!(cited(&run, &json!([{ "claim": "x", "call": "c2" }])).is_err(), "a failed call was cited");
    assert!(cited(&run, &json!([{ "claim": "x", "call": "nope" }])).is_err(), "a made-up call was cited");
    assert!(cited(&run, &json!([{ "claim": "", "call": "c1" }])).is_err());
    assert_eq!(cited(&run, &Value::Null), Ok(vec![]));
}

/// The card's status line: the plan's step when there is one, else what it
/// is doing now; none once it has ended.
#[test]
fn the_status_line_follows_the_plan_or_the_last_action() {
    let mut s = started();
    call(&mut s, 2, "c1", "mail__get_sent", json!({}), "model", "done", false);
    assert_eq!(status_line(&s.runs["r1"]).as_deref(), Some("Get sent in mail"));
    let plan = json!({ "units": [
        { "id": "a", "goal": "Reading the support inbox", "status": "done" },
        { "id": "b", "goal": "Drafting replies", "status": "active" },
        { "id": "c", "goal": "Sending", "status": "pending" } ] });
    s.apply(&ev(4, "kernel", json!({ "kind": "plan", "run": "r1", "version": 1, "plan": plan })));
    assert_eq!(status_line(&s.runs["r1"]).as_deref(), Some("Drafting replies · step 2 of 3"));
    s.apply(&ev(5, "kernel", json!({ "kind": "completed", "run": "r1", "summary": "done" })));
    assert_eq!(status_line(&s.runs["r1"]), None);
}

#[test]
fn a_tool_name_that_starts_with_a_wide_letter_is_said_without_a_panic() {
    assert_eq!(doing("éditer_fiche"), "Éditer fiche");
    assert_eq!(doing("crm__ébauche"), "Ébauche in crm");
    assert_eq!(doing(""), "");
}
