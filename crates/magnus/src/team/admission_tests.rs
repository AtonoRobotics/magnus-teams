//! One rule for what a run is offered and what its call may do
//! (2026-11-14-one-admission), read through the server: every kind of run
//! is offered a tool exactly when the boundary's policy stage admits it,
//! and a team request's record call is refused rather than carried out.

use crate::admin_app::tests::server_with;
use crate::run::admission::Refusal;
use axum::Router;
use serde_json::{json, Value};

fn config() -> Value {
    json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 7, "max_actions": 9, "max_usd": 3.0 }, "verifier": false, "instructions": "", "harness": "" })
}

fn denied(r: Result<(), Refusal>) -> String {
    match r {
        Err(Refusal::Denied { why, .. }) => why,
        other => panic!("not denied: {other:?}"),
    }
}

/// For a run of every kind the server starts, the tools offered are
/// exactly the tools of its catalog that the boundary's policy stage
/// admits, in the catalog's order.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_is_offered_a_tool_exactly_when_its_call_would_be_admitted() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let (st, v) = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();
    let channel = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
    let (request, _) = s.k.team_request(&ws, &channel, None, "sam", "what is on the launch page".into()).await.unwrap();
    let emit = |actor: &'static str, e: Value| {
        let k = s.k.clone();
        async move { k.emit(actor, serde_json::from_value(e).unwrap()).await.unwrap() }
    };
    let created =
        |task: &str, chat: Value| json!({ "kind": "task_created", "task": task, "run": format!("r_{task}"), "person": "sam", "text": "x", "config": config(), "chat": chat });
    emit("person:sam", created("turn", json!({ "thread": "th", "turn": true }))).await;
    emit("person:sam", created("plain", Value::Null)).await;
    emit("schedule:sc1", created("sched", json!({ "thread": "schedule-sc1", "turn": false }))).await;
    emit("review", created("review", Value::Null)).await;
    emit("kernel", json!({ "kind": "sub_agent", "child": "r_child", "parent": "r_plain", "tools": ["web_fetch", "memory_search"] })).await;
    emit("run:r_plain", created("child", Value::Null)).await;
    let mut runs: Vec<String> = ["r_turn", "r_plain", "r_sched", "r_review", "r_child"].map(String::from).to_vec();
    runs.push(s.k.read(|st| st.tasks[&request].run_id.clone()));
    for id in runs {
        let run = s.k.read(|st| st.runs.get(&id).cloned()).unwrap();
        let catalog = s.k.tool_catalog(&run, true).await;
        let offered: Vec<String> = s.k.every_tool_for(&run).await.into_iter().map(|t| t.name).collect();
        let admitted: Vec<String> = catalog.iter().filter(|t| s.k.admits(&run, &t.name).is_ok()).map(|t| t.name.clone()).collect();
        assert_eq!(offered, admitted, "{id}: offered is not what the boundary admits");
        assert!(!offered.is_empty(), "{id} was offered nothing");
    }
    let sched = s.k.read(|st| st.runs["r_sched"].clone());
    assert!(denied(s.k.admits(&sched, "create_schedule")).contains("breed"), "a scheduled run may set up a schedule");
    let child = s.k.read(|st| st.runs["r_child"].clone());
    assert_eq!(
        s.k.every_tool_for(&child).await.into_iter().filter(|t| !crate::tools::RECORDS.contains(&t.name.as_str())).count(),
        2,
        "a sub-agent was offered more than it was given"
    );
    s.done().await;
}

/// A record call a team request makes is refused, not carried out: the
/// run is not ended by a Done it was never offered.
#[tokio::test(flavor = "multi_thread")]
async fn a_team_requests_record_call_is_refused() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let ws = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await.1["id"].as_str().unwrap().to_string();
    let channel = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
    let (task, _) = s.k.team_request(&ws, &channel, None, "sam", "what is on the launch page".into()).await.unwrap();
    let run = s.k.read(|st| st.runs.get(&st.tasks[&task].run_id).cloned().unwrap());
    let flow = s.k.record_call(&run, "c1", "s1", "task_complete", &json!({ "summary": "done", "deliverable": [] })).await.unwrap();
    assert!(matches!(flow, crate::run::Flow::Continue), "the record call ended the run");
    let status = s.k.read(|st| st.runs[&run.id].status);
    assert_ne!(status, crate::state::Status::Completed, "a team request was completed by a record it was not offered");
    s.done().await;
}
