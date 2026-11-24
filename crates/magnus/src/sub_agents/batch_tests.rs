//! A batch of delegation calls in one reply is admitted before any of it
//! starts: the cap on sub-agents and the action budget hold for a batch
//! run at once as for calls made one by one.

use super::*;
use crate::admin_app::tests::{server_with, Server};
use crate::kernel::Kernel;
use crate::sub_agents::MAX_CHILDREN_PER_RUN;
use axum::Router;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

/// Longer than a batch of background starts takes.
const DRIVE_WAIT: Duration = Duration::from_secs(60);

async fn emit(k: &Kernel, actor: &str, e: Value) {
    k.emit(actor, serde_json::from_value(e).unwrap()).await.unwrap();
}

/// mo's run `rt` with `max_actions`, whose latest reply asks for `n`
/// background sub-agents at once; it is driven until it stops. What the
/// drive said comes back too.
async fn batch_of(s: &Server, n: usize, max_actions: u32) -> (Arc<Kernel>, String) {
    // A sub-agent is told the scope recalled from the person's words.
    s.store().await.batch_execute(&crate::log::content_walls_sql("postgres")).await.unwrap();
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 9, "max_actions": max_actions, "max_usd": 4.0 }, "verifier": false, "instructions": "", "harness": "" });
    emit(&s.k, "person:mo", json!({ "kind": "task_created", "task": "t_rt", "run": "rt", "person": "mo", "text": "Check the books.", "config": config })).await;
    let calls: Vec<Value> = (1..=n)
        .map(|i| json!({ "type": "tool_call", "id": format!("c{i}"), "name": "start_sub_agent", "input": { "goal": format!("Check ledger {i}."), "background": true } }))
        .collect();
    let usage = json!({ "input_tokens": 1, "output_tokens": 1, "cost_usd": 0.0 });
    emit(&s.k, "model", json!({ "kind": "model_reply", "run": "rt", "step": 1, "blocks": calls, "usage": usage, "served_model": "m", "stop": "tool_use" })).await;
    let k = s.k.clone();
    let driver = tokio::spawn(k.clone().drive("rt"));
    let said = format!("{:?}", tokio::time::timeout(DRIVE_WAIT, driver).await);
    (k, said)
}

/// The run's own sub-agents, and the calls refused with their stage, and
/// every call's result for a failure's message.
fn outcome(k: &Kernel, n: usize) -> (usize, Vec<(String, String)>, String) {
    k.read(|st| {
        let children = st.sub_agents.below("rt").into_iter().filter(|(d, _)| *d == 1).count();
        let results = &st.runs["rt"].results;
        let refused = (1..=n).map(|i| format!("c{i}")).filter_map(|c| results.get(&c).filter(|r| r.is_error && r.text.contains("Denied (")).map(|r| (c, r.text.clone()))).collect();
        let all = (1..=n).map(|i| format!("c{i}: {:?}", results.get(&format!("c{i}")).map(|r| &r.text))).collect::<Vec<_>>().join("\n");
        (children, refused, all)
    })
}

/// The loop's futures are deep: the server runs its workers on stacks of
/// this size, and so do these tests.
const WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

fn on_big_stacks(f: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_multi_thread().enable_all().thread_stack_size(WORKER_STACK_BYTES).build().unwrap().block_on(f)
}

/// Twelve `start_sub_agent` calls in one reply start eight sub-agents, the
/// most one run may start; the last four are refused before any starts.
#[test]
fn a_batch_of_twelve_starts_eight_and_refuses_four() {
    on_big_stacks(async {
        let s = server_with("", |_| Router::new()).await;
        let (k, said) = batch_of(&s, 12, 40).await;
        let (children, refused, all) = outcome(&k, 12);
        assert_eq!(children, MAX_CHILDREN_PER_RUN, "a batch started past the cap on sub-agents, or fewer: {said}\n{all}");
        let ids: Vec<&str> = refused.iter().map(|(c, _)| c.as_str()).collect();
        assert_eq!(ids, ["c9", "c10", "c11", "c12"], "the calls past the cap were not the ones refused:\n{all}");
        assert!(refused.iter().all(|(_, t)| t.contains("Denied (sub_agents)")), "{refused:?}");
        s.done().await;
    })
}

/// A batch cannot take more actions than the run has left: with three
/// left, three of five calls go ahead and two are refused.
#[test]
fn a_batch_takes_no_more_actions_than_are_left() {
    on_big_stacks(async {
        let s = server_with("", |_| Router::new()).await;
        let (k, said) = batch_of(&s, 5, 3).await;
        let (children, refused, all) = outcome(&k, 5);
        let actions = k.read(|st| crate::spend::tree::used(st, "rt").actions);
        assert!(actions <= 3, "a batch took {actions} actions of a budget of 3");
        assert_eq!(children, 3, "{said}\n{all}");
        let ids: Vec<&str> = refused.iter().map(|(c, _)| c.as_str()).collect();
        assert_eq!(ids, ["c4", "c5"], "{all}");
        assert!(refused.iter().all(|(_, t)| t.contains("Denied (budget)")), "{refused:?}");
        s.done().await;
    })
}

#[test]
fn a_batch_goes_ahead_as_far_as_both_limits_allow() {
    assert_eq!(fit(3, Ok(8), 40), (3, None));
    let (go, why) = fit(12, Ok(8), 40);
    assert_eq!((go, why.map(|(stage, _)| stage)), (8, Some("sub_agents")));
    let (go, why) = fit(5, Ok(8), 3);
    assert_eq!((go, why.map(|(stage, _)| stage)), (3, Some("budget")));
    let (go, why) = fit(2, Err("Sub-agents cannot be nested this deeply.".into()), 9);
    assert_eq!((go, why), (0, Some(("sub_agents", "Sub-agents cannot be nested this deeply".to_string()))));
    // No action left: none is refused here, the boundary stops the run.
    assert_eq!(fit(4, Ok(8), 0), (0, None));
}
