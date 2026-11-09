//! Home: opening the app shows what changed since the person's last
//! session — ended tasks, decided cards and new arrivals — alongside
//! their queued observations, and items the app presents are recorded
//! opened in the log (#367).

use magnus_invariants::model::Script;
use magnus_invariants::*;
use serde_json::{json, Value};
use std::time::Duration;

const REVIEW: &str = "reviewing one person's state";

/// Reviews observe once, then finish; anything else fails fast.
fn model() -> Script {
    script(|c| async move {
        if !c.body["system"].to_string().contains(REVIEW) {
            return tool("task_fail", json!({ "reason": "the backup drive is full" }));
        }
        if c.turn() == 0 {
            return tool("observe", json!({ "what": "Something happened", "why": "It matters", "evidence": "somewhere" }));
        }
        tool("task_complete", json!({ "summary": "Reviewed quietly.", "deliverable": [{ "type": "text", "text": "Reviewed." }] }))
    })
}

async fn drain_reviews(bob: &Api) {
    eventually("all reviews settled", Duration::from_secs(60), || {
        let me = bob.clone();
        async move {
            let (_, v) = me.get("/v1/tasks").await;
            let reviews: Vec<_> = v["tasks"].as_array().cloned().unwrap_or_default().into_iter().filter(|t| t["review"] == true).collect();
            (!reviews.is_empty() && reviews.iter().all(|t| t["status"] == "completed" || t["status"] == "failed")).then_some(())
        }
    })
    .await;
}

async fn wait_observation(bob: &Api) -> Value {
    eventually("an open observation", Duration::from_secs(60), || {
        let me = bob.clone();
        async move { me.inbox().await.into_iter().find(|i| i["kind"] == "observation" && i["status"] == "open" && i["person"] == "bob") }
    })
    .await
}

async fn home(bob: &Api) -> Value {
    let (st, v) = bob.post("/v1/home/opened", json!({})).await;
    assert!(st.is_success(), "{st} {v}");
    v
}

/// Opening home shows the failed task, the observation's arrival and
/// the queued observation; opening it again shows nothing new, until
/// the observation is decided. Presenting the observation records it
/// opened.
#[tokio::test(flavor = "multi_thread")]
async fn home_shows_what_changed_and_tracks_opened() {
    let m = FakeModel::start(model()).await;
    let app = Appliance::start(Spec::default().proactive(json!({ "on": true })), &m).await;
    let bob = app.api("bob");
    drain_reviews(&bob).await;
    let o = wait_observation(&bob).await;
    let oid = o["id"].as_str().unwrap().to_string();
    let tid = bob.submit("Back up the photos.").await;
    let settled = bob.wait_settled(&tid).await;
    assert_eq!(settled["status"], "failed", "{settled}");
    // The failure's own review observes and settles before the first
    // open, so nothing lands between the opens below.
    drain_reviews(&bob).await;

    let first = home(&bob).await;
    assert!(
        first["ended"].as_array().cloned().unwrap_or_default().iter().any(|t| t["task_id"].as_str() == Some(tid.as_str()) && t["status"] == "failed"),
        "the failed task is not in the feed: {first}"
    );
    assert!(first["arrived"].as_array().cloned().unwrap_or_default().iter().any(|i| i["id"] == oid), "the observation's arrival is not in the feed: {first}");
    assert!(first["observations"].as_array().cloned().unwrap_or_default().iter().any(|i| i["id"] == oid), "the queued observation is not in the feed: {first}");
    assert_eq!(first["decided"].as_array().map(|d| d.len()), Some(0), "nothing is decided yet: {first}");

    let (st, v) = bob.post("/v1/inbox/opened", json!({ "ids": [oid] })).await;
    assert!(st.is_success(), "{st} {v}");
    assert_eq!(v["opened"], json!([oid]), "the presented item was not recorded: {v}");

    let second = home(&bob).await;
    for section in ["ended", "arrived", "decided"] {
        assert_eq!(second[section].as_array().map(|s| s.len()), Some(0), "a second open shows {section} again: {second}");
    }
    let seen: Vec<_> = second["observations"].as_array().cloned().unwrap_or_default().into_iter().filter(|i| i["id"] == oid).collect();
    assert_eq!(seen.len(), 1, "the queued observation left the feed: {second}");
    assert!(seen[0]["opened_at"].is_string(), "presenting it did not record opened: {second}");

    assert!(bob.respond(&oid, json!({ "decision": "dismiss" })).await.is_success());
    let third = home(&bob).await;
    assert!(third["decided"].as_array().cloned().unwrap_or_default().iter().any(|i| i["id"] == oid), "the dismissal is not in the feed: {third}");
    assert!(third["observations"].as_array().cloned().unwrap_or_default().iter().all(|i| i["id"] != oid), "the decided observation still queues: {third}");
}
