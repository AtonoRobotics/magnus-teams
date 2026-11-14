//! A workspace's and a channel's spend limits hold across team runs: what
//! open runs may still spend is held for them when the next one starts, and
//! a run stops at its next model call once a limit is spent, a sub-agent of
//! it too.

use super::spend::TeamLimited;
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

fn run_of(s: &Server, task: &str) -> Run {
    s.k.read(|st| st.runs.get(&st.tasks[task].run_id).cloned().unwrap())
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
    s.k.emit("kernel", Ev::MemoryPass { run: run.into(), model: "m".into(), blocks: vec![], usage }).await.unwrap();
}

/// The run asked to go on, as a person resuming it does.
async fn resume(s: &Server, run: &str) {
    s.k.emit("person:sam", Ev::Resumed { run: run.into() }).await.unwrap();
    s.k.wake(run);
}

#[tokio::test(flavor = "multi_thread")]
async fn two_starts_cannot_both_take_what_a_workspace_has_left() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let (ws, channel) = workspace(&s, &sam).await;
    limit(&s, &sam, &ws, json!({ "usd": 1.0 })).await;

    let (first, _) = s.k.team_request(&ws, &channel, None, "sam", "summarise the launch page".into()).await.unwrap();
    assert_eq!(run_of(&s, &first).config.budget.max_usd, 1.0, "the first start was not given what is left");
    // Nothing is spent yet, but the first run may still spend all of it.
    let second = s.k.team_request(&ws, &channel, None, "sam", "and the pricing page".into()).await;
    let e = second.expect_err("a second start was given the dollar the first run holds");
    assert!(e.is::<TeamLimited>() && e.to_string().contains("This workspace"), "{e:#}");
    s.done().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn two_starts_cannot_both_take_what_a_channel_has_left() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let (ws, channel) = workspace(&s, &sam).await;
    limit(&s, &sam, &ws, json!({ "usd": 1.5, "channel": channel })).await;

    let (first, _) = s.k.team_request(&ws, &channel, None, "sam", "summarise the launch page".into()).await.unwrap();
    assert_eq!(run_of(&s, &first).config.budget.max_usd, 1.5);
    charge(&s, &run_of(&s, &first).id, 0.5).await;
    // Spent 0.5, and 1.0 more is the first run's to spend: nothing is left.
    let e = s.k.team_request(&ws, &channel, None, "sam", "and the pricing page".into()).await.expect_err("a second start was given what the first run holds");
    assert!(e.is::<TeamLimited>() && e.to_string().contains("This channel"), "{e:#}");
    // Once the owner raises the limit, the next start gets only the raise.
    limit(&s, &sam, &ws, json!({ "usd": 2.0, "channel": channel })).await;
    let (third, _) = s.k.team_request(&ws, &channel, None, "sam", "and the pricing page".into()).await.unwrap();
    assert!((run_of(&s, &third).config.budget.max_usd - 0.5).abs() < 1e-9, "{}", run_of(&s, &third).config.budget.max_usd);
    s.done().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_team_run_and_its_sub_agent_stop_once_the_workspace_limit_is_lowered_past_its_spend() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let (ws, channel) = workspace(&s, &sam).await;
    limit(&s, &sam, &ws, json!({ "usd": 10.0 })).await;
    let (task, run) = s.k.team_request(&ws, &channel, None, "sam", "summarise the launch page".into()).await.unwrap();
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
    let (_, one) = s.k.team_request(&ws, &channel, None, "sam", "summarise the launch page".into()).await.unwrap();
    limit(&s, &sam, &ws, json!({ "usd": 20.0, "channel": channel })).await;
    let (_, two) = s.k.team_request(&ws, &channel, None, "sam", "and the pricing page".into()).await.unwrap();
    let before = stopped(&s, &two).await;
    assert_ne!(before.reason.as_deref(), Some("spend_limit"), "stopped for the limit before it was reached");

    // The first run's last call overran its budget, and with it the channel's.
    charge(&s, &one, 20.0).await;
    resume(&s, &two).await;
    let after = stopped(&s, &two).await;
    assert_eq!(after.reason.as_deref(), Some("spend_limit"), "a team run went on past its channel's spent limit");
    s.done().await;
}

/// Two mentions at once: the second start reads what is left only after
/// the first has recorded the run that claims it.
#[tokio::test(flavor = "multi_thread")]
async fn two_starts_at_once_cannot_both_take_what_is_left() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let (ws, channel) = workspace(&s, &sam).await;
    limit(&s, &sam, &ws, json!({ "usd": 1.0 })).await;
    let (a, b) =
        tokio::join!(s.k.team_request(&ws, &channel, None, "sam", "summarise the launch page".into()), s.k.team_request(&ws, &channel, None, "sam", "and the pricing page".into()));
    let started = [a, b].into_iter().filter_map(Result::ok).map(|(task, _)| run_of(&s, &task).config.budget.max_usd).sum::<f64>();
    assert!(started <= 1.0, "starts at once were given ${started} of a $1 limit");
    s.done().await;
}
