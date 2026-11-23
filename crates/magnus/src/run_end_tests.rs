//! Ending a run ends its sub-agents: a cancelled task or schedule, and a
//! run that ended any other way, leave no child of theirs going, and a
//! child whose parent has ended asks the model nothing more. Before, only
//! the top run was cancelled and its children went on under its authority.
use crate::admin_app::tests::server_with;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::Status;
use axum::Router;
use serde_json::{json, Value};
use std::sync::Arc;

fn ev(v: Value) -> Ev {
    serde_json::from_value(v).unwrap()
}

fn created(task: &str, run: &str) -> Ev {
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 9, "max_actions": 9, "max_usd": 4.0 }, "verifier": false, "instructions": "", "harness": "" });
    ev(json!({ "kind": "task_created", "task": task, "run": run, "person": "mo", "text": "Check the books.", "config": config }))
}

/// mo's run `top` (task `t_<top>`), started as `actor`, with three
/// sub-agents: two of its own and one below the first. None is driven.
async fn tree(k: &Arc<Kernel>, actor: &str, top: &str) -> Vec<String> {
    k.emit(actor, created(&format!("t_{top}"), top)).await.unwrap();
    let kids = [(format!("{top}_a"), top.to_string()), (format!("{top}_b"), top.to_string()), (format!("{top}_c"), format!("{top}_a"))];
    for (child, parent) in &kids {
        k.emit("kernel", ev(json!({ "kind": "sub_agent", "child": child, "parent": parent, "tools": [] }))).await.unwrap();
        k.emit(&format!("run:{parent}"), created(&format!("t_{child}"), child)).await.unwrap();
    }
    kids.into_iter().map(|(c, _)| c).collect()
}

/// Every child ended cancelled, and asked the model nothing and did
/// nothing, even when its driver comes round again.
async fn ended_without_a_step(k: &Arc<Kernel>, kids: &[String]) {
    for c in kids {
        let _ = k.clone().drive(c).await;
        let r = k.read(|s| s.runs[c].clone());
        assert_eq!(r.status, Status::Cancelled, "sub-agent {c} went on after the run above it ended");
        assert_eq!((r.model_calls, r.actions), (0, 0), "sub-agent {c} asked the model or acted after the run above it ended");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_task_ends_its_sub_agents() {
    let s = server_with("", |_| Router::new()).await;
    let kids = tree(&s.k, "person:mo", "rt").await;
    let mo = s.k.person("mo").unwrap();
    s.k.cancel_task(&mo, "t_rt", None).await.unwrap();
    ended_without_a_step(&s.k, &kids).await;
    s.done().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_schedule_ends_the_sub_agents_of_its_run() {
    let s = server_with("", |_| Router::new()).await;
    s.k.emit("person:mo", ev(json!({ "kind": "schedule_set", "id": "sc1", "person": "mo", "text": "Check the books.", "every_minutes": 60 }))).await.unwrap();
    let kids = tree(&s.k, "schedule:sc1", "rs").await;
    s.k.emit("kernel", ev(json!({ "kind": "schedule_fired", "id": "sc1", "task": "t_rs" }))).await.unwrap();
    let mo = s.k.person("mo").unwrap();
    let out = s.k.cancel_schedule(&mo, "sc1", None).await.unwrap();
    assert_eq!(out["runs_stopped"], json!(1), "{out}");
    assert_eq!(s.k.read(|st| st.runs["rs"].status), Status::Cancelled);
    ended_without_a_step(&s.k, &kids).await;
    s.done().await;
}

/// A run that ended some other way: a child the backstop meets before its
/// next step ends there, and the run's end cancels the rest.
#[tokio::test(flavor = "multi_thread")]
async fn a_sub_agent_takes_no_step_once_a_run_above_it_has_ended() {
    let s = server_with("", |_| Router::new()).await;
    let kids = tree(&s.k, "person:mo", "rd").await;
    s.k.emit("kernel", ev(json!({ "kind": "completed", "run": "rd", "summary": "Done." }))).await.unwrap();
    // Its driver comes round before the run's end is acted on.
    ended_without_a_step(&s.k, &kids[1..2]).await;
    assert_eq!(s.k.read(|st| st.runs["rd_a"].status), Status::Running, "the backstop reached past the run it met");
    s.k.on_stop("rd", true).await;
    ended_without_a_step(&s.k, &kids).await;
    s.done().await;
}
