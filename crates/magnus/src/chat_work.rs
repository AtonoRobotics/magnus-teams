//! In chat Magnus does the work itself (spec, "Chat and tasks"): a chat
//! turn reads, writes, makes files, uses its computer, starts sub-agents and
//! sends, each boundary action reviewed like any run's. A task is for work
//! on a schedule, in the background, or that outlasts the conversation.
//!
//! The sub-agents a turn starts work inside its conversation: they are shown
//! there, what each is doing, its status and its result, and are not listed
//! as the person's tasks.
use crate::boundary::{is_chat_tool, Facts};
use crate::kernel::Kernel;
use crate::recall::Words;
use crate::state::{Run, State, Task};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

/// Whether a chat turn is offered this tool from the built-in set: every
/// built-in action. A task's run records (its contract, plan and
/// completion) are not built-in actions, so they stay out: a turn ends with
/// its answer.
pub(crate) fn offered_in_chat(name: &str) -> bool {
    crate::tools::builtin(name).is_some()
}

/// Why the boundary refuses a call for where it was made, if so: the chat
/// tools are for chat. A chat turn may otherwise use any tool, checked by
/// policy, review and approval as any run's call is.
pub(crate) fn misplaced(chat_turn: bool, f: &Facts) -> Option<String> {
    (is_chat_tool(f) && !chat_turn && !IN_ANY_RUN.contains(&f.name.as_str()))
        .then(|| format!("{} is for chat; in a task, do the work, or suggest it to the person with suggest", f.name))
}

/// The chat tools a task or a review is offered too: a schedule never asks
/// on its own (spec rev 142, Permissions and tasks), so work outside chat
/// sets one up and sees the person's. `suggest` is not a chat tool at all.
const IN_ANY_RUN: [&str; 2] = ["create_schedule", "list_schedules"];

/// Whether a run outside chat is offered this chat tool. A scheduled run
/// sets up no schedule: schedules would breed (2026-10-30).
pub(crate) fn in_any_run(name: &str, scheduled: bool) -> bool {
    name == "suggest" || (IN_ANY_RUN.contains(&name) && !(scheduled && name == "create_schedule"))
}

impl Kernel {
    /// A member's request in a team thread: it talks as a chat turn does,
    /// answering in its thread with text, with no contract, plan or Done.
    /// A sub-agent it starts is a task like any sub-agent.
    pub fn team_talks(&self, run: &Run) -> bool {
        self.team_of(&run.task).is_some_and(|t| !t.sub)
    }

    /// Whether the run ends with its answer: a chat turn, or a team request.
    pub fn converses(&self, run: &Run) -> bool {
        self.is_chat_turn(run) || self.team_talks(run)
    }
}

impl State {
    /// The chat turn a sub-agent works inside: the run at the top of its
    /// parents, when that run is a chat turn.
    pub fn chat_turn_above(&self, run: &str) -> Option<&Run> {
        let top = self.runs.get(&self.sub_agents.top(run)?)?;
        self.tasks.get(&top.task).filter(|t| t.is_chat_turn()).map(|_| top)
    }

    /// Shown in a conversation rather than listed as a task: a chat turn,
    /// and a sub-agent working inside one.
    pub fn in_chat(&self, t: &Task) -> bool {
        t.is_chat_turn() || self.chat_turn_above(&t.run_id).is_some()
    }
}

/// The sub-agents a chat turn started, nested ones too, as one entry of its
/// conversation placed when the first began: each one's goal, status, what
/// it waits on and its result.
pub(crate) fn sub_agents_entry(s: &State, turn: &Task, r: &Run, w: &Words) -> Option<(DateTime<Utc>, i64, Value)> {
    let mut first: Option<DateTime<Utc>> = None;
    let agents: Vec<Value> = s
        .sub_agents
        .below(&r.id)
        .into_iter()
        .filter_map(|(depth, c)| {
            let cr = s.runs.get(&c.child)?;
            let ct = s.tasks.get(&cr.task)?;
            first = Some(first.map_or(ct.created_at, |f| f.min(ct.created_at)));
            Some(json!({
                "run": c.child, "parent": c.parent, "depth": depth, "task_id": ct.task_id, "goal": w.text(ct.seq),
                "status": crate::api::status_str(cr.status), "waiting_on": cr.reason, "waiting_item": cr.waiting_item,
                "summary": cr.summary, "tools": c.tools, "at": ct.created_at, "ended_at": cr.ended_at,
            }))
        })
        .collect();
    let at = first?;
    Some((at, 1, json!({ "id": format!("s:{}", turn.task_id), "kind": "sub_agents", "task_id": turn.task_id, "from_run": r.id, "agents": agents, "at": at })))
}

impl Kernel {
    pub(crate) fn sub_agents_entry(&self, turn: &Task, r: &Run, w: &Words) -> Option<(DateTime<Utc>, i64, Value)> {
        self.read(|s| sub_agents_entry(s, turn, r, w))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::Event;
    use crate::tools::{Audience, Label, Risk};

    fn facts(name: &str, app: &str, risk: Risk) -> Facts {
        Facts { name: name.into(), app: app.into(), risk, label: Label::PRINCIPAL, audience: Audience::Private, connector: None, remote: name.into(), reaches_self: false }
    }

    /// Chat does the work any chat agent does: it is offered the actions
    /// that change things, the computer and sub-agents, not only reads.
    #[test]
    fn a_chat_turn_is_offered_the_work_itself() {
        for t in ["sandbox_exec", "sandbox_write_file", "deliver_file", "browser_click", "browser_type", "computer_click", "start_sub_agent", "knowledge_add", "web_fetch"] {
            assert!(offered_in_chat(t), "{t} is not offered in chat");
        }
        for t in ["task_contract", "task_plan", "task_complete", "task_fail"] {
            assert!(!offered_in_chat(t), "{t}, a task's record, was offered in chat");
        }
    }

    /// A change made in chat is not refused for being in chat: it meets
    /// policy, review and approval like any run's. Only the chat tools stay
    /// in chat.
    #[test]
    fn a_chat_turn_may_change_things() {
        for f in
            [facts("sandbox_exec", "sandbox", Risk::Sandbox), facts("browser_click", "browser", Risk::ReversibleWrite), facts("knowledge_add", "knowledge", Risk::ReversibleWrite)]
        {
            assert_eq!(misplaced(true, &f), None, "{} was refused in chat", f.name);
        }
        assert!(misplaced(false, &facts("create_task", "chat", Risk::ReversibleWrite)).is_some(), "a task used a chat tool");
        assert_eq!(misplaced(false, &facts("create_schedule", "chat", Risk::Read)), None, "a task cannot set up a schedule");
        assert_eq!(misplaced(false, &facts("list_schedules", "chat", Risk::Read)), None, "a task cannot see the schedules");
        assert!(in_any_run("create_schedule", false) && in_any_run("list_schedules", true) && in_any_run("suggest", true));
        assert!(!in_any_run("create_schedule", true), "a scheduled run is offered create_schedule");
        assert!(!in_any_run("create_task", false) && !in_any_run("cancel_schedule", false));
        assert_eq!(misplaced(true, &facts("create_task", "chat", Risk::ReversibleWrite)), None);
    }

    /// The instructions a turn runs under tell it to do the work, not to
    /// hand it to a task.
    #[test]
    fn the_chat_instructions_say_to_do_the_work() {
        let p = crate::run::CHAT_INSTRUCTIONS;
        assert!(!p.contains("may only read"), "chat is still told it may only read");
        assert!(p.contains("start_sub_agent"), "chat is not told it can start sub-agents");
        assert!(p.contains("Do the work yourself"), "chat is not told to do the work itself");
    }

    fn ev(seq: i64, actor: &str, ev: Value) -> Event {
        Event { seq, at: chrono::Utc::now(), actor: actor.into(), ev: serde_json::from_value(ev).unwrap() }
    }

    fn created(seq: i64, task: &str, run: &str, chat: Value) -> Event {
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
        ev(seq, "person:alice", json!({ "kind": "task_created", "task": task, "run": run, "person": "alice", "text": "x", "config": config, "chat": chat }))
    }

    /// A sub-agent a chat turn starts, and one it starts in turn, work in
    /// that chat: shown there with their status and result, not listed as
    /// tasks. One a task starts stays with that task.
    #[test]
    fn sub_agents_of_a_chat_turn_show_in_its_conversation() {
        let mut s = State::default();
        s.apply(&created(1, "turn", "rt", json!({ "thread": "th", "turn": true })));
        s.apply(&created(2, "c1", "r1", Value::Null));
        s.apply(&ev(3, "kernel", json!({ "kind": "sub_agent", "child": "r1", "parent": "rt", "tools": [] })));
        s.apply(&created(4, "c2", "r2", Value::Null));
        s.apply(&ev(5, "kernel", json!({ "kind": "sub_agent", "child": "r2", "parent": "r1", "tools": ["web_fetch"] })));
        s.apply(&ev(6, "kernel", json!({ "kind": "completed", "run": "r2", "summary": "Found three flights." })));
        s.apply(&created(7, "job", "rj", Value::Null));
        s.apply(&created(8, "c3", "r3", Value::Null));
        s.apply(&ev(9, "kernel", json!({ "kind": "sub_agent", "child": "r3", "parent": "rj", "tools": [] })));
        assert!(s.in_chat(&s.tasks["c1"]) && s.in_chat(&s.tasks["c2"]), "a chat turn's sub-agent was listed as a task");
        assert!(!s.in_chat(&s.tasks["c3"]) && !s.in_chat(&s.tasks["job"]), "a task's sub-agent left its task");
        let (_, _, e) = sub_agents_entry(&s, &s.tasks["turn"], &s.runs["rt"], &Words::default()).expect("no entry for the turn's sub-agents");
        assert_eq!(e["kind"], "sub_agents");
        let agents = e["agents"].as_array().unwrap();
        assert_eq!(agents.iter().map(|a| (a["run"].as_str().unwrap(), a["depth"].as_u64().unwrap())).collect::<Vec<_>>(), [("r1", 1), ("r2", 2)]);
        assert_eq!((agents[0]["status"].as_str(), agents[1]["status"].as_str()), (Some("running"), Some("completed")));
        assert_eq!(agents[1]["summary"], "Found three flights.");
        assert!(sub_agents_entry(&s, &s.tasks["job"], &s.runs["rj"], &Words::default()).is_some_and(|(_, _, e)| e["agents"].as_array().unwrap().len() == 1));
    }
}
