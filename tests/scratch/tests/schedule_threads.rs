//! Each schedule run posts to the schedule's own thread (behavior spec,
//! Tasks and schedules in conversation: "Each schedule run posts to the
//! schedule's own thread, so recurring output doesn't bury the
//! conversation"). The thread is in the person's list, holds each run and
//! its result, takes a reply like any other, and the conversation the
//! schedule was set up in gets none of it.

use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;
use std::time::Duration;

const CHAT: &str = "talking with the person you work for in a chat";
const SWEEP: &str = "Sweep the shared mailbox for invoices.";

#[tokio::test(flavor = "multi_thread")]
async fn each_schedule_run_posts_to_the_schedules_own_thread() {
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
    let thread = format!("schedule-{}", schedule["id"].as_str().unwrap());

    // Its first run posts its result in the schedule's own thread.
    let result = eventually("the run's result in the schedule's thread", Duration::from_secs(120), || async {
        let (st, c) = alice.get(&format!("/v1/chat/{thread}")).await;
        if !st.is_success() {
            return None;
        }
        c["entries"].as_array().unwrap().iter().find(|e| e["kind"] == "result").cloned()
    })
    .await;
    assert_eq!((result["status"].as_str(), result["summary"].as_str()), (Some("failed"), Some("No invoices today.")), "{result}");
    assert_eq!(result["text"], SWEEP, "{result}");
    // The thread is in the person's list, under what the schedule does.
    let (_, list) = alice.get("/v1/chat").await;
    let listed =
        list["threads"].as_array().unwrap().iter().find(|t| t["thread"] == thread.as_str()).cloned().unwrap_or_else(|| panic!("the schedule's thread is not listed: {list}"));
    assert!(listed["title"].as_str().is_some_and(|t| t.contains("Sweep the shared mailbox")), "{listed}");
    // The conversation it was set up in gets none of its runs.
    let (_, c) = alice.get(&format!("/v1/chat/{origin}")).await;
    assert!(!c["entries"].as_array().unwrap().iter().any(|e| e["kind"] == "result" || e["kind"] == "tasks"), "a run posted to the conversation the schedule came from: {c}");
    // The person can reply in it.
    let (st, v) = alice.post("/v1/chat", json!({ "text": "Thanks, keep going.", "thread": thread })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    assert_eq!(v["thread"], thread.as_str());
    // Nobody else sees it.
    assert_eq!(app.api("bob").get(&format!("/v1/chat/{thread}")).await.0.as_u16(), 404);
}
