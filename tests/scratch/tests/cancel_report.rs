//! A cancel says what had already happened: a task cancel names side
//! effects still in flight or of unknown outcome as possibly done,
//! irreversible first, and a schedule cancel reports what each stopped
//! run had done and counts only runs actually stopped (#263).

use magnus_invariants::{mcp, *};
use magnus_scratch::{reconfigure, start_checked};
use serde_json::{json, Value};
use std::time::Duration;

/// Long enough for a schedule due every minute to fire.
const MINUTE: Duration = Duration::from_secs(100);

/// Drafts the person can send; the send is irreversible and gated on its
/// recipients like Gmail's.
async fn drafts_app(model: &FakeModel) -> (FakeMcp, Appliance) {
    let id_only = json!({ "type": "object", "properties": { "draft_id": { "type": "string" } } });
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
        .tool("send_draft", "Send a draft.", id_only, |w, a, _| {
            let d = w["drafts"][a["draft_id"].as_str().unwrap_or_default()].clone();
            w["sent"].as_array_mut().unwrap().push(d);
            mcp::ok(json!({ "status": "sent" }))
        })
        .start()
        .await;
    let spec = Spec::default().app("drafts", &mcp, "public", "private", &[("get_draft", "read"), ("send_draft", "irreversible")]).destinations(
        "drafts",
        "send_draft",
        "get_draft",
        "draft_id",
        &["/message/to"],
        &["/message/subject", "/message/body"],
    );
    let app = Appliance::start(spec, model).await;
    let policy = permit_execute("drafts__send_draft");
    assert!(app.api("alice").put_policy("org", &org_policy(&policy)).await.is_success());
    (mcp, app)
}

async fn tasks_with(api: &Api, text: &str) -> Vec<Value> {
    api.tasks().await["tasks"].as_array().unwrap().iter().filter(|t| t["text"] == text).cloned().collect()
}

/// Schedules may repeat every minute, and never pause on failures.
async fn every_minute(app: &mut Appliance) {
    reconfigure(app, |c| {
        let mut t = toml::Table::new();
        t.insert("min_every_minutes".into(), 1.into());
        t.insert("pause_after_failures".into(), 0.into());
        c.insert("schedules".into(), toml::Value::Table(t));
    })
    .await;
}

/// Cancelling with a send in flight names it possibly done: it may have
/// gone out, and it cannot be taken back.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_names_an_in_flight_send_possibly_done() {
    let model = start_checked(script(|c| async move {
        match c.turn() {
            0 => plan("send Bob the Q3 draft"),
            1 => tool("drafts__send_draft", json!({ "draft_id": "d1" })),
            _ => complete("sent"),
        }
    }))
    .await;
    let (drafts, app) = drafts_app(&model).await;
    let alice = app.api("alice");
    let send = drafts.hold("send_draft");
    let task = alice.submit("Send Bob the Q3 draft.").await;
    tokio::time::timeout(PATIENCE, send.received()).await.expect("the send never reached the app");

    let (st, v) = alice.post(&format!("/v1/tasks/{task}/cancel"), json!({})).await;
    assert!(st.is_success(), "cancel: {st} {v}");
    assert_eq!(v["possibly_done"], json!(["drafts: send draft"]), "{v}");
    assert!(v["cannot_be_undone"].as_array().is_some_and(|l| l.is_empty()), "{v}");
    assert!(v["already_done"].as_array().is_some_and(|l| l.is_empty()), "{v}");
    send.release();
}

/// Cancelling a schedule reports what each stopped run had done, and
/// counts only runs actually stopped.
#[tokio::test(flavor = "multi_thread")]
async fn a_schedule_cancel_reports_what_each_stopped_run_had_done() {
    let model = start_checked(script(|c| async move {
        match c.turn() {
            0 => plan("send Bob the Q3 draft"),
            1 => tool("drafts__send_draft", json!({ "draft_id": "d1" })),
            2 => tool("ask_principal", json!({ "question": "Send it again next quarter?" })),
            _ => complete("sent"),
        }
    }))
    .await;
    let (_drafts, mut app) = drafts_app(&model).await;
    every_minute(&mut app).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/schedules", json!({ "text": "Send Bob the Q3 draft.", "every_minutes": 1 })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let id = v["id"].as_str().unwrap().to_string();

    let first =
        eventually("the schedule's first run", MINUTE, || async { tasks_with(&alice, "Send Bob the Q3 draft.").await.first().map(|t| t["task_id"].as_str().unwrap().to_string()) })
            .await;
    alice.wait_item(&first, "question").await;

    let (st, v) = alice.send(reqwest::Method::DELETE, &format!("/v1/schedules/{id}"), None).await;
    assert_eq!(st.as_u16(), 200, "cancel: {st} {v}");
    assert_eq!(v["runs_stopped"], 1, "{v}");
    assert_eq!(v["runs"].as_array().map(|l| l.len()), Some(1), "{v}");
    assert_eq!(v["runs"][0]["task"], first.as_str(), "{v}");
    assert_eq!(v["runs"][0]["cannot_be_undone"], json!(["drafts: send draft"]), "{v}");
}
