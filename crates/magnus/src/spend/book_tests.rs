//! The money ledger from plain events: spend outlives the trash and an
//! erasure, folds the same on replay, and every limit bounds what a call
//! may spend, with what calls in flight hold.

use crate::log::Event;
use crate::spend::allowance::{allowance, allowance_held, scope_of_run, Spender};
use crate::spend::enforce::left_now;
use crate::spend::holds::Hold;
use crate::spend::month;
use crate::state::State;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{json, Value};

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 20, 12, 0, 0).single().unwrap()
}

fn event(seq: i64, actor: &str, e: Value) -> Event {
    Event { seq, at: now(), actor: actor.into(), ev: serde_json::from_value(e).unwrap() }
}

/// ada's task `task_<n>` and its run `r<n>`, with a $50 budget.
fn task(n: i64, person: &str) -> Vec<Event> {
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 9, "max_actions": 9, "max_usd": 50.0 }, "verifier": false, "instructions": "", "harness": "" });
    vec![event(
        n * 10,
        &format!("person:{person}"),
        json!({ "kind": "task_created", "task": format!("task_{n}"), "run": format!("r{n}"), "person": person, "text": "x", "config": config }),
    )]
}

fn cost(seq: i64, run: &str, usd: f64) -> Event {
    let usage = json!({ "input_tokens": 1, "output_tokens": 1, "cost_usd": usd });
    event(seq, "model", json!({ "kind": "model_reply", "run": run, "step": 0, "usage": usage, "served_model": "m", "stop": "end" }))
}

fn finished(seq: i64, run: &str) -> Event {
    event(seq, "kernel", json!({ "kind": "completed", "run": run, "summary": "done" }))
}

fn limit(seq: i64, scope: Value, usd: f64) -> Event {
    event(seq, "person:admin", json!({ "kind": "spend", "change": "limit_set", "scope": scope, "usd": usd }))
}

/// `task_1` as a team run in workspace `w`, channel `c`.
fn team(seq: i64, channel_limit: f64) -> Vec<Event> {
    vec![
        event(seq, "person:ada", json!({ "kind": "team", "change": "created", "workspace": "w", "name": "W", "owner": "ada" })),
        event(seq + 1, "person:ada", json!({ "kind": "team", "change": "run", "workspace": "w", "channel": "c", "thread": null, "task": "task_1", "requester": "ada" })),
        event(seq + 2, "person:ada", json!({ "kind": "team", "change": "chat", "workspace": "w", "chat": { "what": "spend_limit", "channel": "c", "usd": channel_limit } })),
    ]
}

fn fold(events: &[Event]) -> State {
    let mut s = State::default();
    for e in events {
        s.apply(e);
    }
    s
}

/// A person at their monthly limit trashes the task that spent it, and
/// then removes it for good: the limit still refuses, as it does after a
/// replay of the same log.
#[test]
fn trashing_a_task_does_not_give_its_spend_back() {
    let mut log = vec![limit(1, json!({ "person": "ada" }), 10.0)];
    log.extend(task(1, "ada"));
    log.extend([cost(11, "r1", 10.0), finished(12, "r1")]);
    let mut s = fold(&log);
    assert!(left_now(&s, "ada", now()).is_err());
    for e in [
        event(13, "person:ada", json!({ "kind": "trashed", "item": "task_1", "person": "ada" })),
        event(14, "person:ada", json!({ "kind": "removed", "item": "task_1", "seqs": [] })),
    ] {
        s.apply(&e);
        log.push(e);
        assert!(!s.runs.contains_key("r1"), "the run is still there");
        assert!(left_now(&s, "ada", now()).is_err(), "trashing the task gave its spend back");
    }
    let m = month::key(now());
    assert_eq!(month::spent(&fold(&log), &m), month::spent(&s, &m), "a replay folds other totals");
}

/// A trashed team task's spend still counts against its channel.
#[test]
fn trashing_a_team_task_does_not_give_its_channels_spend_back() {
    let mut log = task(1, "ada");
    log.extend(team(2, 1.0));
    log.extend([cost(11, "r1", 1.0), finished(12, "r1"), event(13, "person:ada", json!({ "kind": "trashed", "item": "task_1", "person": "ada" }))]);
    let s = fold(&log);
    let spent = crate::team::spend::spent_this_month(&s, "w", now());
    assert_eq!((spent.total_usd, spent.channels_usd.get("c").copied()), (1.0, Some(1.0)), "the channel's spend went with the task");
}

/// Erasing a person takes their name off their spend, not the spend: the
/// server's month still counts it, and nothing of the ledger names them.
#[test]
fn erasing_a_person_keeps_their_spend_without_their_name() {
    let mut log = vec![limit(1, json!("server"), 10.0)];
    log.extend(task(1, "zed"));
    log.extend(task(2, "amy"));
    log.extend([cost(21, "r1", 9.0), cost(22, "r2", 1.0), event(23, "admin:root", json!({ "kind": "person_erased", "person": "zed" }))]);
    let s = fold(&log);
    assert!(left_now(&s, "amy", now()).is_err_and(|e| e.which == "server"), "erasing zed gave the server's month back");
    let book = format!("{:?}", s.spend);
    assert!(!book.contains("zed"), "the ledger still names zed: {book}");
    let sp = month::spent(&s, &month::key(now()));
    assert_eq!((sp.total_usd, sp.by_person.get("amy").copied(), sp.by_person.len()), (10.0, Some(1.0), 1));
}

/// A call's allowance is the least any limit has left: here the channel's
/// one dollar, though the run's budget and the month have more.
#[test]
fn a_channel_with_a_dollar_left_gives_a_call_at_most_a_dollar() {
    let mut log = task(1, "ada");
    log.extend(team(2, 3.0));
    log.extend([limit(5, json!({ "person": "ada" }), 20.0), cost(11, "r1", 2.0)]);
    let left = allowance(&fold(&log), "r1", Spender::Step, now()).unwrap();
    assert_eq!((left.usd, left.of), (1.0, "channel"));
}

/// The person's month binds too, and an extended budget does not lift it.
#[test]
fn an_extended_budget_does_not_pass_the_month() {
    let mut log = task(1, "ada");
    log.extend([limit(5, json!({ "person": "ada" }), 3.0), cost(11, "r1", 2.5)]);
    let budget = json!({ "max_model_calls": 18, "max_actions": 18, "max_usd": 100.0 });
    log.push(event(12, "person:ada", json!({ "kind": "budget_extended", "run": "r1", "budget": budget })));
    let left = allowance(&fold(&log), "r1", Spender::Step, now()).unwrap();
    assert_eq!((left.usd, left.of), (0.5, "person"));
}

/// While one run's call is in flight, what it holds is not left for
/// another run of the same person: together they cannot pass the month.
#[test]
fn what_a_call_in_flight_holds_is_not_left_for_another() {
    let mut log = task(1, "ada");
    log.extend(task(2, "ada"));
    log.push(limit(30, json!({ "person": "ada" }), 1.0));
    let s = fold(&log);
    let held = [Hold { usd: 0.8, ..scope_of_run(&s, "r1") }];
    let left = allowance_held(&s, "r2", Spender::Step, now(), &held).unwrap();
    assert!((left.usd - 0.2).abs() < 1e-9 && left.of == "person", "{left:?}");
    let bob = Hold { usd: 0.8, person: Some("bob".into()), runs: vec!["r9".into()], ..Hold::default() };
    assert_eq!(allowance_held(&s, "r2", Spender::Step, now(), &[bob]).unwrap().usd, 1.0, "bob's call held ada's month");
}
