//! Every kind of run against every question asked of it, from a log; and
//! the two places authority was read off the run's own task instead of
//! through its parents (2026-11-13-run-kind).

use super::{OutcomeReturn, Prompt, RunKind};
use crate::events::Ev;
use crate::log::Event;
use crate::state::State;
use serde_json::{json, Value};

fn apply(s: &mut State, actor: &str, ev: Ev) {
    let seq = s.last_seq + 1;
    s.apply(&Event { seq, at: chrono::Utc::now(), actor: actor.into(), ev });
}

fn ev(s: &mut State, actor: &str, e: Value) {
    apply(s, actor, serde_json::from_value(e).unwrap());
}

/// A task and its run `r_{task}`; a returning one carries `returns_outcome`.
fn task(s: &mut State, actor: &str, task: &str, chat: Value, returns: bool) {
    let modules = if returns { json!({ crate::outcomes::MODULE: true }) } else { json!({}) };
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 7, "max_actions": 9, "max_usd": 3.0 }, "verifier": false, "instructions": "", "harness": "", "modules": modules });
    ev(s, actor, json!({ "kind": "task_created", "task": task, "run": format!("r_{task}"), "person": "carol", "text": "x", "config": config, "chat": chat }));
}

fn team(s: &mut State, task: &str, sub: bool) {
    let run = crate::team::Change::Run {
        workspace: "ws".into(),
        channel: "ch".into(),
        thread: None,
        task: task.into(),
        requester: "carol".into(),
        history: String::new(),
        sub,
        outside: false,
    };
    apply(s, "kernel", Ev::Team(run));
}

/// A sub-agent `r_{task}` of `parent`, recorded before its task as
/// `start_sub_agent` records it; a team run's child is a team run too.
fn sub(s: &mut State, parent: &str, task: &str, in_team: bool) {
    ev(s, "kernel", json!({ "kind": "sub_agent", "child": format!("r_{task}"), "parent": parent, "tools": [] }));
    self::task(s, &format!("run:{parent}"), task, Value::Null, false);
    if in_team {
        team(s, task, true);
    }
}

/// One of every kind, and a sub-agent of each top kind that differs.
fn every_kind() -> State {
    let mut s = State::default();
    task(&mut s, "person:carol", "turn", json!({ "thread": "th", "turn": true }), false);
    task(&mut s, "chat:r_turn", "fromchat", json!({ "thread": "th", "turn": false, "from_run": "r_turn" }), true);
    task(&mut s, "outcome:fromchat", "oturn", json!({ "thread": "th", "turn": true }), false);
    task(&mut s, "person:carol", "request", Value::Null, false);
    team(&mut s, "request", false);
    task(&mut s, "poll", "routine", Value::Null, false);
    team(&mut s, "routine", false);
    task(&mut s, "person:carol", "plain", Value::Null, false);
    task(&mut s, "schedule:sc1", "sched", json!({ "thread": "schedule-sc1", "turn": false }), true);
    task(&mut s, "review", "review", Value::Null, false);
    task(&mut s, "poll", "poll", Value::Null, false);
    sub(&mut s, "r_turn", "sub_turn", false);
    sub(&mut s, "r_review", "sub_review", false);
    sub(&mut s, "r_routine", "sub_routine", true);
    sub(&mut s, "r_request", "sub_request", true);
    sub(&mut s, "r_sched", "sub_sched", false);
    // A sub-agent's own sub-agent has the kind of the run at the top.
    sub(&mut s, "r_sub_routine", "sub_sub_routine", true);
    s
}

/// The answers to every question, in the order of the table below.
#[derive(Debug, PartialEq)]
struct Answers {
    chat: bool,
    converses: bool,
    records: bool,
    prompt: Prompt,
    background: bool,
    scheduled: bool,
    unattended: bool,
    team: bool,
    wakes_review: bool,
    label: &'static str,
    outcome: Option<OutcomeReturn>,
}

fn answers(s: &State, run: &str) -> Answers {
    let k = s.run_kind(run);
    Answers {
        chat: k.is_chat_turn(),
        converses: k.converses(),
        records: k.offers_records(),
        prompt: k.prompt(),
        background: k.is_background(),
        scheduled: k.under_schedule(),
        unattended: k.unattended(),
        team: k.in_team(),
        wakes_review: s.end_wakes_review(run),
        label: k.label(),
        outcome: s.outcome_return(run),
    }
}

#[test]
fn every_kind_answers_every_question() {
    use OutcomeReturn::*;
    use Prompt::{Chat, Review, Task, Team};
    let s = every_kind();
    let sub_of = |k: RunKind| RunKind::SubAgent(Box::new(k));
    #[rustfmt::skip]
    let table: Vec<(&str, RunKind, Answers)> = vec![
        // run, kind, then (chat turn, converses, records, prompt, background,
        // under schedule, unattended, team, end wakes review, label, outcome)
        ("r_turn", RunKind::ChatTurn, a(true, true, false, Chat, false, false, false, false, false, "chat", None)),
        ("r_oturn", RunKind::OutcomeTurn, a(true, true, false, Chat, false, false, false, false, false, "chat", None)),
        ("r_request", RunKind::TeamRequest, a(false, true, false, Team, false, false, false, true, true, "task", None)),
        ("r_routine", RunKind::TeamRoutine, a(false, true, false, Team, true, false, true, true, false, "background", None)),
        ("r_plain", RunKind::Task, a(false, false, true, Task, false, false, false, false, true, "task", None)),
        ("r_fromchat", RunKind::Task, a(false, false, true, Task, false, false, false, false, false, "task", Some(ToConversation))),
        ("r_sched", RunKind::Scheduled, a(false, false, true, Task, false, true, true, false, false, "scheduled", Some(ToScheduleThread))),
        ("r_review", RunKind::Review, a(false, false, true, Review, true, false, true, false, false, "background", None)),
        ("r_poll", RunKind::Poll, a(false, false, true, Task, true, false, true, false, false, "background", None)),
        ("r_sub_turn", sub_of(RunKind::ChatTurn), a(false, false, true, Task, false, false, false, false, false, "sub_agent", Some(ToParent))),
        ("r_sub_review", sub_of(RunKind::Review), a(false, false, true, Task, true, false, true, false, false, "sub_agent", Some(ToParent))),
        ("r_sub_routine", sub_of(RunKind::TeamRoutine), a(false, false, true, Task, true, false, true, true, false, "sub_agent", Some(ToParent))),
        ("r_sub_sub_routine", sub_of(RunKind::TeamRoutine), a(false, false, true, Task, true, false, true, true, false, "sub_agent", Some(ToParent))),
        ("r_sub_request", sub_of(RunKind::TeamRequest), a(false, false, true, Task, false, false, false, true, false, "sub_agent", Some(ToParent))),
        ("r_sub_sched", sub_of(RunKind::Scheduled), a(false, false, true, Task, false, true, true, false, false, "sub_agent", Some(ToParent))),
        // A run the log does not hold answers as a plain task.
        ("r_gone", RunKind::Task, a(false, false, true, Task, false, false, false, false, true, "task", None)),
    ];
    for (run, kind, want) in table {
        assert_eq!(s.run_kind(run), kind, "{run} is the wrong kind");
        assert_eq!(answers(&s, run), want, "{run} answers wrongly");
    }
    // Only a scheduled run's failure keeps its own notice in the inbox.
    assert!([ToParent, ToConversation].iter().all(|o| o.replaces_notice()) && !ToScheduleThread.replaces_notice());
}

#[allow(clippy::too_many_arguments)]
fn a(
    chat: bool,
    converses: bool,
    records: bool,
    prompt: Prompt,
    background: bool,
    scheduled: bool,
    unattended: bool,
    team: bool,
    wakes_review: bool,
    label: &'static str,
    outcome: Option<OutcomeReturn>,
) -> Answers {
    Answers { chat, converses, records, prompt, background, scheduled, unattended, team, wakes_review, label, outcome }
}

/// A sub-agent of a team routine asks no one, as its routine does: it was
/// read off its own task, which is no background work, so it put an ask in
/// front of a person the routine has none of.
#[test]
fn a_sub_agent_of_a_team_routine_asks_no_one() {
    let s = every_kind();
    assert!(s.asks_no_one("r_routine"), "a team routine asks a person");
    assert!(s.asks_no_one("r_sub_routine"), "a sub-agent of a team routine asks a person");
    assert!(s.asks_no_one("r_sub_sub_routine"), "a nested sub-agent of a team routine asks a person");
    for run in ["r_review", "r_poll", "r_sub_review", "r_plain", "r_turn", "r_request", "r_sub_request", "r_sched"] {
        assert!(!s.asks_no_one(run), "{run} may not ask its person");
    }
}

/// A sub-agent of a review left waiting on a question is ended with it, as
/// the review would be, and its card is put away; one waiting on an ask
/// waits on. It was read off its own task, so it was left waiting forever.
#[test]
fn a_sub_agent_of_a_review_left_on_a_question_is_settled() {
    let mut s = every_kind();
    sub(&mut s, "r_review", "sub_asks", false);
    for (task, item_kind, reason) in [("sub_review", "question", "question"), ("sub_asks", "approval", "approval"), ("plain", "question", "question")] {
        let (run, item) = (format!("r_{task}"), format!("i_{task}"));
        ev(&mut s, "kernel", json!({ "kind": "item", "id": item, "item_kind": item_kind, "task": task, "run": run, "person": "carol", "summary": "?" }));
        ev(&mut s, "kernel", json!({ "kind": "suspended", "run": run, "reason": reason, "item": item }));
    }
    let (runs, items) = s.background_to_settle();
    assert_eq!(runs, vec!["r_sub_review".to_string()], "a sub-agent of a review was left waiting on its person");
    assert_eq!(items, vec!["i_sub_review".to_string()], "a sub-agent of a review left its card up");
}
