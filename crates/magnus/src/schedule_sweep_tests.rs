//! The schedule sweep's plan over a state built from events: what ends,
//! what is held and released, and what a due schedule does.

use crate::log::Event;
use crate::schedule_sweep::{ends, firings, holds, Step};
use crate::state::State;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

const PAUSE_AFTER: u32 = 3;

fn ev(s: &mut State, actor: &str, at: DateTime<Utc>, e: Value) {
    let seq = s.last_seq + 1;
    s.apply(&Event { seq, at, actor: actor.into(), ev: serde_json::from_value(e).unwrap() });
}

fn hours(n: i64) -> chrono::Duration {
    chrono::Duration::hours(n)
}

/// alice's hourly schedule `id`, made `ago` hours before `now`.
fn schedule(s: &mut State, now: DateTime<Utc>, id: &str, ago: i64, more: Value) {
    let mut e = json!({ "kind": "schedule_set", "id": id, "person": "alice", "text": "Summarise my inbox.", "every_minutes": 60 });
    if let (Some(e), Some(more)) = (e.as_object_mut(), more.as_object()) {
        e.extend(more.clone());
    }
    ev(s, "person:alice", now - hours(ago), e);
}

/// A run of schedule `id`, task `task`, started `ago` hours before `now`.
fn run(s: &mut State, now: DateTime<Utc>, id: &str, task: &str, ago: i64) {
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    let at = now - hours(ago);
    ev(s, &format!("schedule:{id}"), at, json!({ "kind": "task_created", "task": task, "run": format!("r_{task}"), "person": "alice", "text": "x", "config": config }));
    ev(s, "kernel", at, json!({ "kind": "schedule_fired", "id": id, "task": task }));
}

fn ids(steps: &[Step]) -> Vec<(String, &'static str)> {
    steps
        .iter()
        .map(|x| match x {
            Step::End { schedule, .. } => (schedule.id.clone(), "end"),
            Step::Hold { schedule, .. } => (schedule.id.clone(), "hold"),
            Step::Release { id } => (id.clone(), "release"),
            Step::Skip { schedule, .. } => (schedule.id.clone(), "skip"),
            Step::PauseFailures { schedule, .. } => (schedule.id.clone(), "pause"),
            Step::Fire { schedule } => (schedule.id.clone(), "fire"),
        })
        .collect()
}

/// Out of runs, or past its end: it ends; one already ended does not again.
#[test]
fn a_schedule_out_of_runs_or_past_its_end_ends_once() {
    let now = Utc::now();
    let mut s = State::default();
    schedule(&mut s, now, "sc_runs", 5, json!({ "max_runs": 1 }));
    run(&mut s, now, "sc_runs", "t1", 4);
    ev(&mut s, "kernel", now, json!({ "kind": "completed", "run": "r_t1", "summary": "ok" }));
    schedule(&mut s, now, "sc_until", 5, json!({ "until": (now - hours(1)).to_rfc3339() }));
    schedule(&mut s, now, "sc_going", 5, json!({}));
    assert_eq!(ids(&ends(&s, now)), vec![("sc_runs".into(), "end"), ("sc_until".into(), "end")]);
    ev(&mut s, "kernel", now, json!({ "kind": "schedule_ended", "id": "sc_runs", "reason": "x" }));
    assert_eq!(ids(&ends(&s, now)), vec![("sc_until".into(), "end")], "an ended schedule ended again");
}

/// A run waiting on its person holds the schedule; once the run no longer
/// waits, the hold is released. Waiting for a time holds nothing.
#[test]
fn a_run_waiting_on_its_person_holds_the_schedule_until_it_no_longer_waits() {
    let now = Utc::now();
    let mut s = State::default();
    schedule(&mut s, now, "sc1", 5, json!({}));
    run(&mut s, now, "sc1", "t1", 1);
    ev(&mut s, "kernel", now, json!({ "kind": "suspended", "run": "r_t1", "reason": "approval" }));
    schedule(&mut s, now, "sc2", 5, json!({}));
    run(&mut s, now, "sc2", "t2", 1);
    ev(&mut s, "kernel", now, json!({ "kind": "suspended", "run": "r_t2", "reason": "timer" }));
    assert_eq!(ids(&holds(&s)), vec![("sc1".into(), "hold")]);
    ev(&mut s, "kernel", now, json!({ "kind": "schedule_paused", "id": "sc1", "reason": "waiting", "task": "t1" }));
    assert!(holds(&s).is_empty(), "a held schedule was held again");
    ev(&mut s, "person:alice", now, json!({ "kind": "resumed", "run": "r_t1" }));
    assert_eq!(ids(&holds(&s)), vec![("sc1".into(), "release")]);
}

/// A due schedule fires, unless its last run is open (skip) or its last
/// runs failed in a row (pause); a paused or not yet due one does nothing.
#[test]
fn a_due_schedule_fires_skips_or_pauses() {
    let now = Utc::now();
    let mut s = State::default();
    schedule(&mut s, now, "a_fire", 5, json!({}));
    schedule(&mut s, now, "b_open", 5, json!({}));
    run(&mut s, now, "b_open", "t_open", 2);
    schedule(&mut s, now, "c_failing", 9, json!({}));
    for (i, task) in ["f1", "f2", "f3"].into_iter().enumerate() {
        run(&mut s, now, "c_failing", task, 8 - 2 * i as i64);
        ev(&mut s, "kernel", now - hours(7 - 2 * i as i64), json!({ "kind": "failed", "run": format!("r_{task}"), "reason": "down", "by": "model" }));
    }
    schedule(&mut s, now, "d_paused", 5, json!({}));
    ev(&mut s, "person:alice", now, json!({ "kind": "schedule_paused", "id": "d_paused", "reason": "by alice" }));
    schedule(&mut s, now, "e_new", 0, json!({}));
    assert_eq!(ids(&firings(&s, now, PAUSE_AFTER)), vec![("a_fire".into(), "fire"), ("b_open".into(), "skip"), ("c_failing".into(), "pause")]);
    assert_eq!(ids(&firings(&s, now, 0)).last(), Some(&("c_failing".into(), "fire")), "a count of 0 paused a schedule");
}
