use crate::admin_app::tests::server_with;
use crate::events::Ev;
use axum::Router;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// Short enough for a test, long enough to tell a wait from none.
const LIMIT: Duration = Duration::from_millis(1500);
/// When the sub-agent finishes, counted from the start of the wait.
const FINISHES_AFTER: Duration = Duration::from_millis(400);

fn ev(v: Value) -> Ev {
    serde_json::from_value(v).unwrap()
}

fn created(task: &str, run: &str) -> Ev {
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 9, "max_actions": 9, "max_usd": 4.0 }, "verifier": false, "instructions": "", "harness": "" });
    ev(json!({ "kind": "task_created", "task": task, "run": run, "person": "mo", "text": "Compare the prices.", "config": config }))
}

/// A chat turn asking after its own working sub-agent waits until that one
/// is done, not answering "still working" for the model to ask again.
/// Another run's sub-agent, or one already done, is answered at once.
#[tokio::test(flavor = "multi_thread")]
async fn asking_after_a_working_sub_agent_waits_for_it_to_finish() {
    let s = server_with("", |_| Router::new()).await;
    s.k.emit("person:mo", created("t_top", "top")).await.unwrap();
    s.k.emit("person:mo", created("t_other", "other")).await.unwrap();
    for (child, parent) in [("kid", "top"), ("stranger", "other")] {
        s.k.emit("kernel", ev(json!({ "kind": "sub_agent", "child": child, "parent": parent, "tools": [] }))).await.unwrap();
        s.k.emit(&format!("run:{parent}"), created(&format!("t_{child}"), child)).await.unwrap();
    }

    let started = Instant::now();
    s.k.await_own_child("top", "t_stranger", LIMIT).await;
    assert!(started.elapsed() < FINISHES_AFTER, "a run waited on a sub-agent that is not its own");

    let k = s.k.clone();
    let finisher = tokio::spawn(async move {
        tokio::time::sleep(FINISHES_AFTER).await;
        k.emit("kernel", ev(json!({ "kind": "completed", "run": "kid", "summary": "Linear is $8." }))).await.unwrap();
    });
    let started = Instant::now();
    s.k.await_own_child("top", "t_kid", LIMIT).await;
    let waited = started.elapsed();
    finisher.await.unwrap();
    assert!(waited >= FINISHES_AFTER, "the ask came back after {waited:?}, before its sub-agent finished");
    assert!(waited < LIMIT, "the ask waited out its limit though its sub-agent finished");

    let started = Instant::now();
    s.k.await_own_child("top", "t_kid", LIMIT).await;
    assert!(started.elapsed() < FINISHES_AFTER, "a finished sub-agent was waited on");
    s.done().await;
}
