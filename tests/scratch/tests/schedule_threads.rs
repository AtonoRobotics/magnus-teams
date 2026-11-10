//! Where a schedule's runs post (owner, 2026-11-10; spec rev 140: "its
//! outcome returns as a new turn to the agent in the conversation where the
//! work or schedule started"). A schedule set up in a conversation posts each
//! run's result there. One set up outside a conversation, in settings, posts
//! in a thread of its own, which is in the person's list and takes a reply
//! like any other.

use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;
use std::time::Duration;

const CHAT: &str = "talking with the person you work for in a chat";
const SWEEP: &str = "Sweep the shared mailbox for invoices.";

/// Every run fails with nothing found; a chat turn sets up the schedule.
async fn sweeping() -> (FakeModel, Appliance) {
    let m = FakeModel::start(script(|c| async move {
        if c.body["system"].to_string().contains(CHAT) {
            let first = c.messages().first().cloned().unwrap_or_default();
            let said = first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string();
            return match (said.contains("invoices"), c.turn()) {
                (true, 0) => tool("create_schedule", json!({ "title": "Invoice sweep", "text": SWEEP, "every_minutes": 1, "scope": "Reads the shared mailbox; changes nothing." })),
                _ => Reply::Text("Noted.".into()),
            };
        }
        tool("task_fail", json!({ "reason": "No invoices today." }))
    }))
    .await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    reconfigure(&mut app, |c| {
        let mut t = toml::Table::new();
        t.insert("min_every_minutes".into(), 1.into());
        c.insert("schedules".into(), toml::Value::Table(t));
    })
    .await;
    (m, app)
}

/// The first run's result in a thread, once it is there.
async fn result_in(api: &Api, thread: &str) -> serde_json::Value {
    eventually("the run's result in the thread", Duration::from_secs(120), || async {
        let (st, c) = api.get(&format!("/v1/chat/{thread}")).await;
        if !st.is_success() {
            return None;
        }
        c["entries"].as_array().unwrap().iter().find(|e| e["kind"] == "result").cloned()
    })
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_schedule_set_up_in_a_conversation_posts_its_runs_there() {
    let (_m, app) = sweeping().await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/chat", json!({ "text": "Every minute, sweep the shared mailbox for invoices." })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let origin = v["thread"].as_str().unwrap().to_string();
    alice.wait_settled(v["task_id"].as_str().unwrap()).await;
    let (_, c) = alice.get(&format!("/v1/chat/{origin}")).await;
    let card = c["entries"].as_array().unwrap().iter().find(|e| e["kind"] == "schedule_card").cloned().expect("no schedule card");
    let (st, v) = alice.post(&format!("/v1/chat/cards/{}", card["card"].as_str().unwrap()), json!({ "decision": "confirm" })).await;
    assert!(st.is_success(), "{v}");
    let schedule = eventually("the schedule", PATIENCE, || async { alice.get("/v1/schedules").await.1["schedules"].as_array().unwrap().first().cloned() }).await;

    // Its first run posts its result in the conversation it was set up in.
    let result = result_in(&alice, &origin).await;
    assert_eq!((result["status"].as_str(), result["summary"].as_str()), (Some("failed"), Some("No invoices today.")), "{result}");
    assert_eq!(result["text"], SWEEP, "{result}");
    // It has no thread of its own.
    let own = format!("schedule-{}", schedule["id"].as_str().unwrap());
    let (_, list) = alice.get("/v1/chat").await;
    assert!(!list["threads"].as_array().unwrap().iter().any(|t| t["thread"] == own.as_str()), "the schedule set up in chat has a thread of its own: {list}");
    // Nobody else sees it.
    assert_eq!(app.api("bob").get(&format!("/v1/chat/{origin}")).await.0.as_u16(), 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_schedule_set_up_in_settings_posts_in_its_own_thread() {
    let (_m, app) = sweeping().await;
    let alice = app.api("alice");
    let (st, schedule) = alice.post("/v1/schedules", json!({ "text": SWEEP, "every_minutes": 1 })).await;
    assert!(st.is_success(), "{schedule}");
    let id = schedule["id"].as_str().or(schedule["schedule"].as_str()).unwrap_or_else(|| panic!("no schedule id: {schedule}")).to_string();
    let thread = format!("schedule-{id}");

    let result = result_in(&alice, &thread).await;
    assert_eq!((result["status"].as_str(), result["summary"].as_str()), (Some("failed"), Some("No invoices today.")), "{result}");
    // The thread is in the person's list, under what the schedule does.
    let (_, list) = alice.get("/v1/chat").await;
    let listed =
        list["threads"].as_array().unwrap().iter().find(|t| t["thread"] == thread.as_str()).cloned().unwrap_or_else(|| panic!("the schedule's thread is not listed: {list}"));
    assert!(listed["title"].as_str().is_some_and(|t| t.contains("Sweep the shared mailbox")), "{listed}");
    // The person can reply in it.
    let (st, v) = alice.post("/v1/chat", json!({ "text": "Thanks, keep going.", "thread": thread })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    assert_eq!(v["thread"], thread.as_str());
    // Nobody else sees it.
    assert_eq!(app.api("bob").get(&format!("/v1/chat/{thread}")).await.0.as_u16(), 404);
}
