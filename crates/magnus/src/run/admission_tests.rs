//! One rule for what a run is offered and what its call may do
//! (2026-11-14-one-admission): for every kind of run, a tool is offered
//! exactly when the boundary would let its call through; and the places
//! the two had drifted apart, each read the safer way.

use super::{Admission, Refusal, Setting, HARD_DENIAL};
use crate::events::Ev;
use crate::log::Event;
use crate::state::State;
use serde_json::{json, Value};

fn ev(s: &mut State, actor: &str, e: Value) {
    let seq = s.last_seq + 1;
    s.apply(&Event { seq, at: chrono::Utc::now(), actor: actor.into(), ev: serde_json::from_value(e).unwrap() });
}

fn config() -> Value {
    json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 7, "max_actions": 9, "max_usd": 3.0 }, "verifier": false, "instructions": "", "harness": "" })
}

/// A task of carol's and its run `r_{task}`.
fn task(s: &mut State, actor: &str, task: &str, chat: Value) {
    ev(s, actor, json!({ "kind": "task_created", "task": task, "run": format!("r_{task}"), "person": "carol", "text": "x", "config": config(), "chat": chat }));
}

/// A team run carol asked for in workspace `ws`, which the log holds no
/// membership of: she is no longer a member.
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
    let seq = s.last_seq + 1;
    s.apply(&Event { seq, at: chrono::Utc::now(), actor: "kernel".into(), ev: Ev::Team(run) });
}

fn sub(s: &mut State, parent: &str, task: &str, tools: &[&str]) {
    ev(s, "kernel", json!({ "kind": "sub_agent", "child": format!("r_{task}"), "parent": parent, "tools": tools }));
    self::task(s, &format!("run:{parent}"), task, Value::Null);
}

fn admits(s: &State, run: &str, tool: &str) -> Result<(), Refusal> {
    let setting = Setting { computers: true, ..Setting::default() };
    Admission::new(s, &s.runs[run], &setting).admits(tool)
}

fn denied(r: Result<(), Refusal>) -> String {
    match r {
        Err(Refusal::Denied { why, .. }) => why,
        other => panic!("not denied: {other:?}"),
    }
}

/// A person's own app named `team_<something>` is theirs anywhere: it was
/// refused nothing at the boundary, but was not offered (offering cut every
/// `team_` name). Only the workspace's own tools wait for a team thread.
#[test]
fn a_persons_app_named_team_something_is_offered_as_it_is_admitted() {
    let mut s = State::default();
    task(&mut s, "person:carol", "plain", Value::Null);
    assert_eq!(admits(&s, "r_plain", "team_drive__list"), Ok(()));
    assert!(denied(admits(&s, "r_plain", "team_post")).contains("team thread"));
}

/// The one team tool a person's own agent has reads a base its owner may:
/// it was offered only to someone in a workspace, but let through for
/// anyone. Now it is refused to someone in none.
#[test]
fn reading_team_knowledge_needs_a_workspace() {
    let mut s = State::default();
    task(&mut s, "person:carol", "plain", Value::Null);
    assert!(denied(admits(&s, "r_plain", "team_read_knowledge")).contains("in no workspace"));
}

/// A team run whose requester has left the workspace was still offered the
/// web, the computer and the team's tools, every call of which the boundary
/// refused. It is offered none of them; its records stay its own.
#[test]
fn a_team_run_for_someone_no_longer_a_member_is_offered_nothing_to_act_with() {
    let mut s = State::default();
    task(&mut s, "person:carol", "routine", Value::Null);
    team(&mut s, "routine", false);
    for tool in ["web_fetch", "team_post", "computer_click"] {
        assert!(denied(admits(&s, "r_routine", tool)).contains("no longer a member"), "{tool}");
    }
}

/// A sub-agent of a team run given only some tools was offered every team
/// tool anyway (they were added after its limit was applied), and the
/// boundary refused them. It is offered only what it was given.
#[test]
fn a_team_sub_agent_is_offered_only_what_it_was_given() {
    let mut s = State::default();
    task(&mut s, "person:carol", "request", Value::Null);
    team(&mut s, "request", false);
    sub(&mut s, "r_request", "child", &["web_fetch"]);
    team(&mut s, "child", true);
    let r = admits(&s, "r_child", "team_post");
    assert!(matches!(&r, Err(Refusal::Denied { why, rule: Some(HARD_DENIAL) }) if why.contains("not given")), "{r:?}");
}

/// A team request ends with its answer, as chat does: it is not offered a
/// task's records, and now a record call it makes anyway is refused rather
/// than carried out. The rich-answer tools are any run's.
#[test]
fn a_team_request_has_no_task_records() {
    let mut s = State::default();
    task(&mut s, "person:carol", "request", Value::Null);
    team(&mut s, "request", false);
    for record in ["task_complete", "task_fail", "task_plan", "wait_until", "show"] {
        assert!(admits(&s, "r_request", record).is_err(), "{record}");
    }
    assert_eq!(admits(&s, "r_request", "ui_catalog"), Ok(()), "a record was judged as a team run's action");
}

/// A run the server's switch holds is offered nothing, and any call of it
/// suspends; a connector's switch takes that connector's tools alone.
#[test]
fn a_switch_takes_the_tools_it_holds() {
    let mut s = State::default();
    task(&mut s, "person:carol", "plain", Value::Null);
    ev(&mut s, "admin:root", json!({ "kind": "halt", "change": "engaged", "scope": { "level": "connector", "id": "mail" }, "name": "Root" }));
    assert!(matches!(admits(&s, "r_plain", "mail__send"), Err(Refusal::Halted { .. })));
    assert_eq!(admits(&s, "r_plain", "web_fetch"), Ok(()));
    ev(&mut s, "admin:root", json!({ "kind": "kill_switch", "engaged": true }));
    assert!(matches!(admits(&s, "r_plain", "web_fetch"), Err(Refusal::Halted { .. })));
}

/// Every name the built-ins offer is a built-in action or a run record:
/// so a chat turn, which may use any tool but a task's records, is offered
/// exactly the built-in actions, as before.
#[test]
fn every_offered_builtin_is_an_action_or_a_record() {
    for t in crate::tools::builtin_specs(true, true, true, true).into_iter().chain(crate::tools::chat_specs()) {
        assert!(crate::tools::builtin(&t.name).is_some() || crate::tools::RECORDS.contains(&t.name.as_str()), "{}", t.name);
    }
}
