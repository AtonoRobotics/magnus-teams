//! A run's allowance, from plain events, for each spender and each rule; and
//! the boundary's budget stage, which asks it, against a booted kernel.

use super::{allowance, Spender};
use crate::log::Event;
use crate::state::State;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{json, Value};

const SPENDERS: [Spender; 3] = [Spender::Step, Spender::Extra, Spender::Action];

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 10, 12, 0, 0).single().unwrap()
}

fn ev(s: &mut State, seq: i64, actor: &str, e: Value) {
    s.apply(&Event { seq, at: now(), actor: actor.into(), ev: serde_json::from_value(e).unwrap() });
}

/// ada's run `r1` of task `t1`: three model calls, two actions and $5.
fn world(more: Value) -> State {
    let mut s = State::default();
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 3, "max_actions": 2, "max_usd": 5.0 }, "verifier": false, "instructions": "", "harness": "" });
    let mut e = json!({ "kind": "task_created", "task": "t1", "run": "r1", "person": "ada", "text": "x", "config": config });
    for (k, v) in more.as_object().cloned().unwrap_or_default() {
        e[k] = v;
    }
    ev(&mut s, 10, "person:ada", e);
    s
}

/// `r1` as a team run in workspace `w`, channel `c`, with these limits.
fn team(s: &mut State, limit: Value) {
    ev(s, 20, "person:ada", json!({ "kind": "team", "change": "created", "workspace": "w", "name": "W", "owner": "ada" }));
    ev(s, 21, "person:ada", json!({ "kind": "team", "change": "run", "workspace": "w", "channel": "c", "thread": null, "task": "t1", "requester": "ada" }));
    let mut chat = json!({ "what": "spend_limit" });
    for (k, v) in limit.as_object().cloned().unwrap_or_default() {
        chat[k] = v;
    }
    ev(s, 22, "person:ada", json!({ "kind": "team", "change": "chat", "workspace": "w", "chat": chat }));
}

fn month_limit(s: &mut State, scope: Value, usd: f64) {
    ev(s, 30, "person:admin", json!({ "kind": "spend", "change": "limit_set", "scope": scope, "usd": usd }));
}

fn spent(s: &mut State, usd: f64) {
    s.runs.get_mut("r1").unwrap().usd = usd;
}

/// Each spender's answer: `ok`, or the stop's reason and the details' `which`.
fn answers(s: &State, run: &str) -> Vec<String> {
    SPENDERS
        .iter()
        .map(|sp| match allowance(s, run, *sp, now()) {
            Ok(_) => "ok".to_string(),
            Err(stop) => match stop.details["which"].as_str() {
                Some(which) => format!("{}:{which}", stop.reason),
                None => stop.reason.to_string(),
            },
        })
        .collect()
}

/// Every rule, and which spenders it binds: the call cap counts the agent's
/// own steps for the agent and every call for an extra; the action cap binds
/// only actions; the dollars, the grant's, the month's and the team's
/// limits bind all three alike.
#[test]
fn one_allowance_holds_every_rule_for_every_spender() {
    type Setup = fn(&mut State);
    let rows: Vec<(&str, Setup, [&str; 3])> = vec![
        ("nothing spent", |_| {}, ["ok", "ok", "ok"]),
        (
            "the agent made its three calls",
            |s| {
                s.runs.get_mut("r1").unwrap().model_calls = 3;
            },
            ["budget", "budget", "ok"],
        ),
        (
            "the kernel's extras made three calls",
            |s| {
                let r = s.runs.get_mut("r1").unwrap();
                (r.model_calls, r.aux_calls) = (3, 3);
            },
            ["ok", "budget", "ok"],
        ),
        (
            "two actions taken",
            |s| {
                s.runs.get_mut("r1").unwrap().actions = 2;
            },
            ["ok", "ok", "budget"],
        ),
        ("its five dollars spent", |s| spent(s, 5.0), ["budget", "budget", "budget"]),
        (
            "ada's month limit spent by this run",
            |s| {
                month_limit(s, json!({ "person": "ada" }), 1.0);
                spent(s, 2.0);
            },
            ["spend_limit:person", "spend_limit:person", "spend_limit:person"],
        ),
        (
            "the server's month limit spent",
            |s| {
                month_limit(s, json!("server"), 1.0);
                spent(s, 1.5);
            },
            ["spend_limit:server", "spend_limit:server", "spend_limit:server"],
        ),
        (
            "the workspace's limit spent",
            |s| {
                team(s, json!({ "usd": 1.0 }));
                spent(s, 1.0);
            },
            ["spend_limit:workspace", "spend_limit:workspace", "spend_limit:workspace"],
        ),
        (
            "the channel's limit spent",
            |s| {
                team(s, json!({ "channel": "c", "usd": 0.5 }));
                spent(s, 0.75);
            },
            ["spend_limit:channel", "spend_limit:channel", "spend_limit:channel"],
        ),
        (
            "a team limit not yet spent",
            |s| {
                team(s, json!({ "usd": 3.0 }));
                spent(s, 1.0);
            },
            ["ok", "ok", "ok"],
        ),
    ];
    for (what, setup, want) in rows {
        let mut s = world(json!({}));
        setup(&mut s);
        assert_eq!(answers(&s, "r1"), want, "{what}");
    }
}

/// A grant's budget for its period, spent by its tasks, stops each spender;
/// once its person resumes the run past it, it no longer does.
#[test]
fn a_spent_grant_stops_every_spender_until_its_person_resumes_past_it() {
    let mut w = world(json!({ "grant": "g1" }));
    ev(&mut w, 11, "person:ada", json!({ "kind": "grant_set", "id": "g1", "person": "ada", "apps": ["notes"], "period": "week", "max_usd": 2.0, "cedar": "" }));
    assert_eq!(answers(&w, "r1"), ["ok"; 3]);
    w.arrangements.grants_mut().get_mut("g1").unwrap().charges.push((now(), 2.0));
    assert_eq!(answers(&w, "r1"), ["grant_budget"; 3]);
    let stop = allowance(&w, "r1", Spender::Step, now()).unwrap_err();
    assert_eq!(stop.notice, "Stopped: the grant g1 has spent its $2.00 for this week ($2.00).");
    w.runs.get_mut("r1").unwrap().past_grant = true;
    assert_eq!(answers(&w, "r1"), ["ok"; 3]);
}

/// A sub-agent is held to the team limits of the run at its top.
#[test]
fn a_sub_agent_is_held_to_its_top_runs_team_limit() {
    let mut s = world(json!({}));
    team(&mut s, json!({ "usd": 1.0 }));
    ev(&mut s, 40, "kernel", json!({ "kind": "sub_agent", "child": "r2", "parent": "r1", "tools": [] }));
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 3, "max_actions": 2, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    ev(&mut s, 41, "run:r1", json!({ "kind": "task_created", "task": "t2", "run": "r2", "person": "ada", "text": "x", "config": config }));
    assert_eq!(answers(&s, "r2"), ["ok"; 3]);
    spent(&mut s, 1.0);
    assert_eq!(answers(&s, "r2"), ["spend_limit:workspace"; 3]);
}

/// The stops say what they always said: the app and the tests read them.
#[test]
fn the_stops_keep_their_words_and_details() {
    let mut s = world(json!({}));
    s.runs.get_mut("r1").unwrap().model_calls = 3;
    let stop = allowance(&s, "r1", Spender::Step, now()).unwrap_err();
    assert_eq!(stop.notice, "Stopped after 3 model calls, the task's limit.");
    assert_eq!(stop.details, json!({ "model_calls": 3, "limit": 3, "usd": 0.0 }));

    let mut s = world(json!({}));
    s.runs.get_mut("r1").unwrap().actions = 2;
    let stop = allowance(&s, "r1", Spender::Action, now()).unwrap_err();
    assert_eq!(stop.notice, "Stopped: budget used: 2 of 2 actions, $0.00 of $5.00.");
    assert_eq!(stop.details, json!({ "actions": 2, "usd": 0.0 }));

    let mut s = world(json!({}));
    month_limit(&mut s, json!({ "person": "ada" }), 1.0);
    spent(&mut s, 2.0);
    let stop = allowance(&s, "r1", Spender::Extra, now()).unwrap_err();
    assert!(stop.notice.starts_with("Stopped: You have reached your spend limit for this month"), "{}", stop.notice);
    assert_eq!((stop.details["can_ask"].as_bool(), stop.details["limit_usd"].as_f64()), (Some(true), Some(1.0)));

    let left = allowance(&world(json!({})), "r1", Spender::Extra, now()).unwrap();
    assert_eq!(left.usd, 5.0);
    assert!(allowance(&world(json!({})), "nope", Spender::Step, now()).is_err(), "a run that is not there has nothing to spend");
}

/// The boundary's budget stage asks the allowance: an action the model asks
/// for stops at the person's month limit and at the team's, not only at the
/// run's own budget.
mod boundary {
    use crate::admin_app::tests::{server_with, Server};
    use crate::boundary::{Action, Issuer, Verdict};
    use crate::events::Ev;
    use crate::kernel::NewRun;
    use crate::spend::{Change, Scope};
    use crate::state::{Run, Status};
    use axum::Router;
    use serde_json::{json, Value};
    use std::time::Duration;

    /// How often, and how many times, a test looks for a run to stop.
    const POLL_EVERY: Duration = Duration::from_millis(20);
    const POLLS: u32 = 500;

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

    /// mo's run, stopped by its driver, then charged `usd` and set running
    /// again with no driver: the next step is the action below.
    async fn charged_run(s: &Server, usd: f64) -> (String, String) {
        let (task, run) = s.k.create_run("person:mo", "mo", "take a note".into(), vec![], NewRun::default()).await.unwrap();
        stopped(s, &run).await;
        let usage = magnus_adapter_proto::Usage { input_tokens: 1, output_tokens: 1, cache_read_tokens: 0, cache_write_tokens: 0, cost_usd: usd };
        s.k.emit("kernel", Ev::MemoryPass { run: run.clone(), model: "m".into(), blocks: vec![], usage }).await.unwrap();
        (task, run)
    }

    async fn team(s: &Server, task: &str, limit_usd: f64) {
        for e in [
            json!({ "kind": "team", "change": "created", "workspace": "w", "name": "W", "owner": "sam" }),
            json!({ "kind": "team", "change": "run", "workspace": "w", "channel": "c", "thread": null, "task": task, "requester": "mo" }),
            json!({ "kind": "team", "change": "chat", "workspace": "w", "chat": { "what": "spend_limit", "usd": limit_usd } }),
        ] {
            s.k.emit("person:sam", serde_json::from_value::<Ev>(e).unwrap()).await.unwrap();
        }
    }

    /// The model's next action, on a run set running again.
    async fn act(s: &Server, run: &str) -> (Verdict, Run) {
        s.k.emit("person:mo", Ev::Resumed { run: run.into() }).await.unwrap();
        let a = Action { run: run.into(), call_id: "c1".into(), step_id: "s9.0".into(), tool: "memory_write".into(), input: json!({ "fact": "x" }), issuer: Issuer::Model };
        let v = s.k.execute(a).await.unwrap();
        (v, s.k.read(|st| st.runs[run].clone()))
    }

    /// What the run's stop said, for a failing assertion.
    fn said(s: &Server, run: &str) -> Value {
        s.k.read(|st| st.runs[run].clone()).summary.map(Value::from).unwrap_or_default()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_action_stops_at_the_persons_month_limit() {
        let s = server_with("", |_| Router::new()).await;
        let (_, run) = charged_run(&s, 2.0).await;
        s.k.emit("person:sam", Ev::Spend(Change::LimitSet { scope: Scope::Person("mo".into()), usd: Some(1.0) })).await.unwrap();
        let (v, r) = act(&s, &run).await;
        assert!(matches!(v, Verdict::Suspended), "an action went on past mo's spent month limit");
        assert_eq!(r.reason.as_deref(), Some("spend_limit"), "{:?}", said(&s, &run));
        s.done().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_action_stops_at_its_workspaces_limit() {
        let s = server_with("", |_| Router::new()).await;
        let (task, run) = charged_run(&s, 3.0).await;
        team(&s, &task, 2.0).await;
        let (v, r) = act(&s, &run).await;
        assert!(matches!(v, Verdict::Suspended), "an action went on past its workspace's spent limit");
        assert_eq!(r.reason.as_deref(), Some("spend_limit"), "{:?}", said(&s, &run));
        s.done().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_action_within_every_limit_is_not_stopped_for_budget() {
        let s = server_with("", |_| Router::new()).await;
        let (task, run) = charged_run(&s, 0.5).await;
        team(&s, &task, 2.0).await;
        s.k.emit("person:sam", Ev::Spend(Change::LimitSet { scope: Scope::Person("mo".into()), usd: Some(1.0) })).await.unwrap();
        let (_, r) = act(&s, &run).await;
        assert_ne!(r.reason.as_deref(), Some("spend_limit"));
        assert_ne!(r.reason.as_deref(), Some("budget"));
        s.done().await;
    }
}
