//! A workspace's and a channel's spend limits hold across the team runs an
//! older release recorded: one that resumes stops at its next model call
//! once a limit is spent, a sub-agent of it too. No team run starts now
//! (2026-11-22-retire-team-agent-runs), so what a start was given went
//! with them.

use super::TeamRun;
use crate::admin_app::tests::{server_with, Server};
use crate::events::Ev;
use crate::kernel::NewRun;
use crate::state::{Run, Status};
use axum::Router;
use serde_json::json;
use std::time::Duration;

/// How often, and how many times, a test looks for a run to stop.
const POLL_EVERY: Duration = Duration::from_millis(20);
const POLLS: u32 = 500;

/// sam's workspace and its first channel.
async fn workspace(s: &Server, sam: &str) -> (String, String) {
    let (st, v) = s.post(sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();
    let channel = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
    (ws, channel)
}

async fn limit(s: &Server, sam: &str, ws: &str, body: serde_json::Value) {
    let (st, v) = s.post(sam, &format!("/v1/workspaces/{ws}/limit"), body).await;
    assert_eq!(st, 200, "{v}");
}

/// The run once its driver has stopped it.
async fn stopped(s: &Server, run: &str) -> Run {
    for _ in 0..POLLS {
        let r = s.k.read(|st| st.runs.get(run).cloned().unwrap());
        if r.status != Status::Running {
            return r;
        }
        tokio::time::sleep(POLL_EVERY).await;
    }
    panic!("run {run} never stopped");
}

/// A model call's cost charged to `run`, as the loop would record it.
async fn charge(s: &Server, run: &str, usd: f64) {
    let usage = magnus_adapter_proto::Usage { input_tokens: 1, output_tokens: 1, cache_read_tokens: 0, cache_write_tokens: 0, cost_usd: usd };
    s.k.emit("kernel", Ev::MemoryPass(crate::events::MemoryPassPayload { run: run.into(), model: "m".into(), blocks: vec![], usage })).await.unwrap();
}

/// The run asked to go on, as a person resuming it does.
async fn resume(s: &Server, run: &str) {
    s.k.emit("person:sam", Ev::Resumed { run: run.into() }).await.unwrap();
    s.k.wake(run);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_team_run_and_its_sub_agent_stop_once_the_workspace_limit_is_lowered_past_its_spend() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let (ws, channel) = workspace(&s, &sam).await;
    limit(&s, &sam, &ws, json!({ "usd": 10.0 })).await;
    let (task, run) = s.k.recorded_team_run(&ws, &channel, "sam", "summarise the launch page").await.unwrap();
    let before = stopped(&s, &run).await;
    assert_ne!(before.reason.as_deref(), Some("spend_limit"), "stopped for the limit before it was reached");

    charge(&s, &run, 3.0).await;
    limit(&s, &sam, &ws, json!({ "usd": 2.0 })).await;
    resume(&s, &run).await;
    let after = stopped(&s, &run).await;
    assert_eq!(after.reason.as_deref(), Some("spend_limit"), "a team run went on past its workspace's lowered limit");

    // A sub-agent is held to its top run's workspace, as start_sub_agent makes one.
    let (child_task, child) = ("task-child".to_string(), "run-child".to_string());
    s.k.emit("kernel", Ev::SubAgent(crate::sub_agents::SubAgent { child: child.clone(), parent: run.clone(), tools: vec![] })).await.unwrap();
    let team = s.k.team_of(&task).map(|t| TeamRun { sub: true, ..t });
    let o = NewRun { team, ids: Some((child_task, child.clone())), budget: Some(crate::config::Budget { max_usd: 1.0, ..s.k.cfg.budget.clone() }), ..Default::default() };
    s.k.create_run(&format!("run:{run}"), "sam", "check the page".into(), vec![], o).await.unwrap();
    let sub = stopped(&s, &child).await;
    assert_eq!(sub.reason.as_deref(), Some("spend_limit"), "a sub-agent went on past its top run's workspace limit");
    s.done().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_team_run_stops_once_another_run_has_spent_its_channels_limit() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let (ws, channel) = workspace(&s, &sam).await;
    limit(&s, &sam, &ws, json!({ "usd": 10.0, "channel": channel })).await;
    let (_, one) = s.k.recorded_team_run(&ws, &channel, "sam", "summarise the launch page").await.unwrap();
    limit(&s, &sam, &ws, json!({ "usd": 20.0, "channel": channel })).await;
    let (_, two) = s.k.recorded_team_run(&ws, &channel, "sam", "and the pricing page").await.unwrap();
    let before = stopped(&s, &two).await;
    assert_ne!(before.reason.as_deref(), Some("spend_limit"), "stopped for the limit before it was reached");

    // The first run's last call overran its budget, and with it the channel's.
    charge(&s, &one, 20.0).await;
    resume(&s, &two).await;
    let after = stopped(&s, &two).await;
    assert_eq!(after.reason.as_deref(), Some("spend_limit"), "a team run went on past its channel's spent limit");
    s.done().await;
}
