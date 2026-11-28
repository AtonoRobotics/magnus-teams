//! Outcomes come back to the agent (spec rev 133, Chat and tasks: "When a
//! task finishes, fails or is blocked, its outcome returns as a new turn to
//! the agent in the conversation or schedule thread that started it").
//!
//! A task started from a conversation, and each run of a schedule, starts
//! with the `returns_outcome` module pinned (K9). When it ends, the sweep
//! starts a chat turn in its thread, once (a derived id, actor
//! `outcome:{task}`). That turn's text is the server's, never the
//! person's: the thread does not show it as their message and later turns do
//! not read it as their words. The agent there owns the goal: on a failure it
//! may start a retry with `create_task`, which runs with the grant, rules,
//! scope and budget the ended task had (never more, and no new approval), up
//! to `[outcomes] max_retry_count` retries; then it tells the person the
//! result, or the one decision or permission it needs.
//!
//! A run blocked on its person in the background already ends as a failure
//! (`settle_background`), so it returns here like any other failure.

use crate::chat::derived;
use crate::events::{ChatOrigin, Ev};
use crate::kernel::{Kernel, NewRun};
use crate::log::Event;
use crate::state::{Run, State, Status, Task};
use crate::tools::Label;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// The module a run starts with when its outcome returns to its thread.
/// Runs from before outcomes returned lack it, so a release that adds it
/// does not wake the agent for every task that ever ended.
pub const MODULE: &str = "returns_outcome";

/// The module a system background task (such as Heartbeat) starts with:
/// it never posts to chat and stays silent unless the person is needed.
pub const SYSTEM_MODULE: &str = "system_task";

/// Whether a schedule's title or text identifies a system Heartbeat task
/// (matches existing Heartbeat schedules created before `system: true`).
pub fn is_heartbeat_text(title: Option<&str>, text: &str) -> bool {
    let matches = |s: &str| {
        let low = s.trim().to_ascii_lowercase();
        !low.is_empty() && (low.starts_with("heartbeat") || low.contains("heartbeat:") || low.split(|c: char| !c.is_alphanumeric()).any(|w| w == "heartbeat"))
    };
    title.is_some_and(matches) || matches(text)
}

/// Whether a task/run is a system background task (e.g. Heartbeat).
pub fn is_system_task(s: &State, t: &Task, r: &Run) -> bool {
    r.config.module(SYSTEM_MODULE) || t.schedule.as_deref().and_then(|id| s.arrangements.schedules().get(id)).is_some_and(|sc| sc.system)
}

/// The most retries configuration may allow: each one spends up to the
/// ended task's whole budget again.
pub const MAX_RETRY_COUNT_CEILING: u32 = 10;

/// Retries after the first attempt, unless configured.
const DEFAULT_MAX_RETRY_COUNT: u32 = 2;

/// Outcomes returned per sweep, so a backlog does not hold the sweep up.
const RETURNS_PER_SWEEP_COUNT: usize = 8;

/// `[outcomes]` in the server's configuration.
#[derive(Deserialize, Clone, Debug)]
pub struct Settings {
    /// How many times the agent may retry a failed task, within the
    /// authority and budget it started with, before it must report to the
    /// person instead.
    #[serde(default = "default_max_retry_count")]
    pub max_retry_count: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { max_retry_count: DEFAULT_MAX_RETRY_COUNT }
    }
}

fn default_max_retry_count() -> u32 {
    DEFAULT_MAX_RETRY_COUNT
}

impl Settings {
    /// Checked when the configuration loads.
    pub fn check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.max_retry_count <= MAX_RETRY_COUNT_CEILING,
            "[outcomes] max_retry_count must be at most {MAX_RETRY_COUNT_CEILING}: each retry may spend the task's whole budget again"
        );
        Ok(())
    }
}

/// What the log says about returned outcomes.
#[derive(Default, Debug)]
pub struct Ledger {
    /// Ended tasks whose outcome has not returned yet.
    due: BTreeSet<String>,
    /// Ended tasks whose outcome returned.
    returned: BTreeSet<String>,
    /// Outcome turns by their task: the task whose outcome each returned.
    turns: BTreeMap<String, String>,
    /// Each schedule's first run, by schedule. Kept here, from the log,
    /// not read from the tasks still listed: a run the person deleted
    /// leaves the list, and the next run is not the first again.
    first_runs: BTreeMap<String, String>,
}

/// Their tasks' outcomes, returned or due: none returns for them after.
impl crate::forget::Forget for Ledger {
    fn forget(&mut self, who: &crate::forget::Erased) {
        self.due.retain(|t| !who.tasks.contains(t));
        self.returned.retain(|t| !who.tasks.contains(t));
        self.turns.retain(|turn, of| !who.tasks.contains(turn) && !who.tasks.contains(of));
        self.first_runs.retain(|_, first| !who.tasks.contains(first));
    }
}

impl Ledger {
    /// The task whose outcome this turn returned, if it is an outcome turn.
    pub fn of(&self, turn: &str) -> Option<&str> {
        self.turns.get(turn).map(String::as_str)
    }

    /// Whether this ended task's outcome still waits to return.
    pub fn is_due(&self, task: &str) -> bool {
        self.due.contains(task)
    }
}

/// Mark a new task's run to return its outcome: a task started from a
/// conversation (its thread, or its schedule's thread), not a chat turn.
pub fn mark(modules: &mut BTreeMap<String, bool>, chat: Option<&ChatOrigin>) {
    if chat.is_some_and(|c| !c.turn) {
        modules.entry(MODULE.into()).or_insert(true);
    }
}

/// Keep the ledger with the log, after `State::apply` took the event.
pub fn observe(s: &mut State, e: &Event) {
    match &e.ev {
        Ev::TaskCreated { task, .. } => {
            if let Some(sc) = e.actor.strip_prefix("schedule:") {
                s.outcomes.first_runs.entry(sc.to_string()).or_insert_with(|| task.clone());
            }
            if let Some(of) = e.actor.strip_prefix("outcome:") {
                s.outcomes.due.remove(of);
                s.outcomes.returned.insert(of.to_string());
                s.outcomes.turns.insert(task.clone(), of.to_string());
            }
        }
        // An outcome with no conversation returned as its notice.
        Ev::Item { item_kind, task: Some(task), details, .. } if item_kind == "notice" && details["reason"] == NOTICE_REASON => {
            s.outcomes.due.remove(task);
            s.outcomes.returned.insert(task.clone());
        }
        // Only the end that stood: a run ends once.
        Ev::Completed { run, .. } | Ev::Failed { run, .. } => {
            let ended = if matches!(e.ev, Ev::Completed { .. }) { Status::Completed } else { Status::Failed };
            let task = s.runs.get(run).filter(|r| r.status == ended && r.config.module(MODULE)).map(|r| r.task.clone());
            // System tasks (e.g. Heartbeat) never notify or post on normal completion —
            // only when the person is needed (failure / waiting). Routines set up outside
            // chat stay silent on completed runs too so they do not leave lingering
            // notifications after setup; a repeating schedule in chat shows its first run
            // and stays silent on later completed runs.
            let task = task.filter(|tid| {
                if ended != Status::Completed {
                    return true;
                }
                let Some(t) = s.tasks.get(tid) else { return false };
                let Some(r) = s.runs.get(&t.run_id) else { return false };
                if is_system_task(s, t, r) {
                    return false;
                }
                if t.schedule.is_some() && notice_only(s, t) {
                    return false;
                }
                !later_run(s, tid)
            });
            if let Some(task) = task.filter(|t| !s.outcomes.returned.contains(t)) {
                s.outcomes.due.insert(task);
            }
        }
        _ => {}
    }
}

/// How many retries came before this task: each step back is a task
/// started from the outcome turn of the one before.
pub fn attempt(s: &State, task: &str) -> u32 {
    let mut n = 0;
    let mut at = task.to_string();
    while n <= MAX_RETRY_COUNT_CEILING {
        let from = s.tasks.get(&at).and_then(|t| t.chat.as_ref()?.from_run.as_ref()).and_then(|r| s.runs.get(r));
        let Some(prev) = from.and_then(|r| s.outcomes.of(&r.task)) else { break };
        n += 1;
        at = prev.to_string();
    }
    n
}

/// The chat turn the person wrote that a task's chain of retries began
/// from: `turn` itself, or, for an outcome turn, the turn that started the
/// task whose outcome it returned, and so on back. An outcome turn's words
/// are the server's, so nothing names a task's request by them.
pub fn person_turn<'a>(s: &'a State, turn: &'a Task) -> &'a Task {
    let mut at = turn;
    for _ in 0..=MAX_RETRY_COUNT_CEILING {
        let from = s.outcomes.of(&at.task_id).and_then(|t| s.tasks.get(t)).and_then(|t| t.chat.as_ref()?.from_run.as_ref()).and_then(|r| s.runs.get(r));
        match from.and_then(|r| s.tasks.get(&r.task)) {
            Some(prev) => at = prev,
            None => break,
        }
    }
    at
}

/// The runs that started a task: a chat turn that started one, an outcome
/// turn that started a retry. What the thread rules below read.
pub fn starters(s: &State) -> BTreeSet<String> {
    s.tasks.values().filter_map(|t| t.chat.as_ref().filter(|c| !c.turn)?.from_run.clone()).collect()
}

// What work shows in its thread (owner, 2026-11-16: "Tasks and schedules do
// not need to post activity. They should post updates only when something
// needs attention or the task is done"): the task on the card of the
// message that started it, and its result once it is done. Not the agent's
// handling of an outcome, a retry starting, nor a result a retry replaced.

/// Whether an outcome turn's reply shows: the task it returned failed and
/// the turn ended without starting a retry, so what it says is the one
/// thing the person must decide.
pub fn reply_shows(s: &State, started: &BTreeSet<String>, of: &str, turn: &Run) -> bool {
    let failed = s.tasks.get(of).and_then(|t| s.runs.get(&t.run_id)).is_some_and(|r| r.status != Status::Completed);
    turn.ended_at.is_some() && failed && !started.contains(&turn.id)
}

/// Whether a task shows on the card of the turn that started it: one the
/// person's own turn started, not a scheduled run nor an outcome turn's retry.
pub fn start_shows(s: &State, t: &Task) -> bool {
    let from = t.chat.as_ref().and_then(|c| c.from_run.as_ref()).and_then(|r| s.runs.get(r));
    from.is_some_and(|r| s.outcomes.of(&r.task).is_none())
}

/// Whether a task is a run of a schedule after its first: one that
/// finished is no news.
fn later_run(s: &State, task: &str) -> bool {
    let schedule = s.tasks.get(task).and_then(|t| t.schedule.as_ref());
    schedule.is_some_and(|sc| s.outcomes.first_runs.get(sc).is_some_and(|first| first != task))
}

/// Whether a task is a quiet routine/system run, which tells no one: any
/// system task (Heartbeat) or any schedule run after its first that does not
/// need its person (it has not failed and waits on no one).
pub fn routine(s: &State, t: &Task, r: &Run) -> bool {
    let waits_on_timer = r.reason.as_deref() == Some("timer");
    let needs_person = r.status == Status::Failed || (r.status == Status::Suspended && !waits_on_timer);
    !needs_person && (is_system_task(s, t, r) || later_run(s, &t.task_id))
}

/// Whether a task's result shows: not one that failed and was retried,
/// whose retry's result is the update, nor a system task or a schedule's
/// later run that finished.
pub fn result_shows(s: &State, started: &BTreeSet<String>, t: &Task) -> bool {
    let run = s.runs.get(&t.run_id);
    let failed = run.is_some_and(|r| r.status != Status::Completed);
    let turn = derived("task", &format!("outcome:{}", t.task_id));
    let retried = s.tasks.get(&turn).is_some_and(|o| started.contains(&o.run_id));
    if failed {
        !retried
    } else if run.is_some_and(|r| is_system_task(s, t, r)) {
        false
    } else {
        !later_run(s, &t.task_id)
    }
}

/// The outcome turn's text: the server's, about the task by id and status
/// only. The task's own words come from the conversation, as a task result
/// carrying its taint, never from here.
pub fn turn_text(task: &str, status: Status, attempt: u32, max: u32) -> String {
    let head = format!(
        "This turn is not a message from the person: the server started it because a task ended. Task {task} {}. The recorded outcome is in the conversation. This follow-up serves the original objective if work remains. ",
        if status == Status::Completed { "finished" } else { "failed" }
    );
    let next = match status {
        Status::Completed => "The result is visible in the thread. This follow-up reply is private to the runtime.".to_string(),
        _ if attempt >= max => format!("Retry limit reached ({attempt} of {max}). The person sees the failure and this reply."),
        _ => {
            format!("A follow-up via create_task would be retry {} of {max}, within the original authority and budget. The person sees this reply if no retry starts.", attempt + 1)
        }
    };
    head + &next
}

/// Whether an ended task's outcome goes to its person as a notice: it has
/// no conversation to return to, or the person deleted it (owner,
/// 2026-11-16: "If it's deleted they show a notification only").
fn notice_only(s: &State, t: &Task) -> bool {
    t.chat.as_ref().is_none_or(|c| s.trash.gone(&c.thread))
}

/// The notice an ended task's outcome sends when it has no conversation:
/// its reason, which also marks the outcome returned (`observe`).
pub const NOTICE_REASON: &str = "task_ended";

/// Ended before this, an outcome due with no conversation sends no notice:
/// tasks whose conversation was deleted before notices were sent would
/// otherwise all announce themselves at once.
const NOTICE_WINDOW_HOURS: i64 = 24;

impl Kernel {
    /// Return each ended task's outcome to its thread, or to its person as a
    /// notice when it has none (the sweep calls this).
    pub async fn return_outcomes(self: &Arc<Self>) {
        let since = chrono::Utc::now() - chrono::Duration::hours(NOTICE_WINDOW_HOURS);
        let due: Vec<(Task, Run, bool)> = self.read(|s| {
            if s.vault.kill_switch() {
                return vec![];
            }
            s.outcomes
                .due
                .iter()
                .filter_map(|id| s.tasks.get(id))
                .filter_map(|t| s.runs.get(&t.run_id).map(|r| (t.clone(), r.clone(), notice_only(s, t))))
                .filter(|(_, r, notice)| !notice || r.ended_at.is_some_and(|e| e > since))
                .take(RETURNS_PER_SWEEP_COUNT)
                .collect()
        });
        for (t, r, notice) in due {
            let done = if notice { self.notify_outcome(&t, &r).await } else { self.return_outcome(&t, &r).await };
            match done {
                Err(e) if !crate::halts::is_refusal(&e) => tracing::warn!("could not return task {}'s outcome: {e:#}", t.task_id),
                _ => {}
            }
        }
    }

    /// An ended task with no conversation: one notice to its person that
    /// it finished or failed, with its title.
    async fn notify_outcome(&self, t: &Task, r: &Run) -> anyhow::Result<()> {
        let w = self.words_of(&t.person, t.seq).await?;
        let title = crate::api::title_of(t, Some(r), &w);
        let summary = match r.status {
            Status::Completed => format!("Finished: {title}"),
            _ => format!("Needs your attention: {title}"),
        };
        let details = serde_json::json!({ "reason": NOTICE_REASON, "status": crate::api::status_str(r.status), "evidence": { "task": t.task_id } });
        let notice = self.notice_to(Some(t.task_id.clone()), Some(r.id.clone()), t.person.clone(), summary, details);
        self.emit("kernel", notice).await?;
        Ok(())
    }

    async fn return_outcome(self: &Arc<Self>, t: &Task, r: &Run) -> anyhow::Result<()> {
        let Some(thread) = t.chat.as_ref().map(|c| c.thread.clone()) else { return Ok(()) };
        let key = format!("outcome:{}", t.task_id);
        let (task, run) = (derived("task", &key), derived("run", &key));
        if self.read(|s| s.tasks.contains_key(&task)) {
            return Ok(());
        }
        let attempt = self.read(|s| attempt(s, &t.task_id));
        let text = turn_text(&t.task_id, r.status, attempt, self.cfg.outcomes.max_retry_count);
        let conversation = self.conversation(&t.person, &thread).await?;
        // It reads what the thread shows, the result among it, so it
        // carries the taint of the runs that wrote it (I2).
        let label = self.thread_label(&t.person, &thread, None);
        let o = NewRun {
            chat: Some(ChatOrigin { thread: thread.clone(), turn: true, from_run: None, conversation, opener: None }),
            label: (label != Label::PRINCIPAL).then_some(label),
            ids: Some((task, run)),
            model: Some((r.config.adapter.clone(), r.config.model.clone())),
            project: self.read(|s| s.project_of(&thread)),
            ..Default::default()
        };
        self.create_run(&key, &t.person, text, vec![], o).await?;
        Ok(())
    }

    /// What a task started from this chat turn may do: from an outcome
    /// turn, a retry within the ended task's authority; otherwise the
    /// person's, or a grant of theirs the model named (#235).
    /// `key` is the call's: one outcome turn starts one task, and the
    /// call resent after a restart finds it again.
    pub(crate) async fn task_authority(&self, run: &Run, turn: &Task, input: &Value, key: &str) -> Result<NewRun, String> {
        let named = input["grant"].as_str().map(str::trim).filter(|g| !g.is_empty());
        if let Some(of) = self.read(|s| s.outcomes.of(&turn.task_id).map(String::from)) {
            let mine = derived("task", key);
            if self.read(|s| s.tasks.values().any(|t| t.task_id != mine && t.chat.as_ref().is_some_and(|c| c.from_run.as_deref() == Some(run.id.as_str())))) {
                return Err("this turn already started its retry; one turn starts one".into());
            }
            return self.retry_authority(&of, named).await;
        }
        // Under a grant: the person's own, live, with budget left; the task
        // spends no more than what is left of it this period.
        let Some(id) = named else { return Ok(NewRun::default()) };
        let (g, left) = self.grant_for_task(&run.person, id)?;
        let mut budget = self.cfg.budget.clone();
        budget.max_usd = budget.max_usd.min(left);
        Ok(NewRun { grant: Some(g.id), budget: Some(budget), ..Default::default() })
    }

    /// A retry of `of`: its grant, scope, modules and budget (within the
    /// server's today and what is left of its grant), up to the limit.
    async fn retry_authority(&self, of: &str, named: Option<&str>) -> Result<NewRun, String> {
        let max = self.cfg.outcomes.max_retry_count;
        let Some((t, r, n)) = self.read(|s| {
            let t = s.tasks.get(of)?;
            Some((t.clone(), s.runs.get(&t.run_id)?.clone(), attempt(s, of)))
        }) else {
            return Err(format!("task {of}, whose outcome this turn returns, is gone"));
        };
        if n >= max {
            return Err(format!("task {of} has reached its retry limit ({n})"));
        }
        if named.is_some() && named != t.grant.as_deref() {
            let had = t.grant.as_deref().map(|g| format!("grant {g}")).unwrap_or_else(|| "no grant".into());
            return Err(format!("a retry runs within the authority task {of} started with ({had}), never more"));
        }
        let mut budget = r.config.budget.within(&self.cfg.budget);
        if let Some(g) = &t.grant {
            let (_, left) = self.grant_for_task(&t.person, g)?;
            budget.max_usd = budget.max_usd.min(left);
        }
        let scope = self.words_of(&t.person, t.seq).await.map_err(|e| format!("{e:#}"))?.opt(t.seq, "scope");
        Ok(NewRun { grant: t.grant.clone(), budget: Some(budget), scope, modules: r.config.modules.clone(), ..Default::default() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(modules: Value) -> Value {
        json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 7, "max_actions": 9, "max_usd": 3.0 }, "verifier": false, "instructions": "", "harness": "", "modules": modules })
    }

    fn ev(s: &mut State, actor: &str, e: Value) {
        let seq = s.last_seq + 1;
        s.apply(&Event { seq, at: chrono::Utc::now(), actor: actor.into(), ev: serde_json::from_value(e).unwrap() });
    }

    fn task(s: &mut State, actor: &str, task: &str, chat: Value, modules: Value) {
        ev(s, actor, json!({ "kind": "task_created", "task": task, "run": format!("r_{task}"), "person": "carol", "text": "x", "config": config(modules), "chat": chat }));
    }

    /// A task from a conversation that ends returns to its thread once; a
    /// cancelled one, a run from before outcomes returned, and a chat turn
    /// do not.
    #[test]
    fn an_ended_task_returns_to_its_thread_once() {
        let mut s = State::default();
        let mut modules = BTreeMap::new();
        mark(&mut modules, Some(&ChatOrigin { thread: "th".into(), turn: false, from_run: Some("r_turn".into()), conversation: vec![], opener: None }));
        assert_eq!(modules.get(MODULE), Some(&true), "a task from chat is not marked to return its outcome");
        let mut turn_modules = BTreeMap::new();
        mark(&mut turn_modules, Some(&ChatOrigin { thread: "th".into(), turn: true, from_run: None, conversation: vec![], opener: None }));
        assert!(turn_modules.is_empty(), "a chat turn would return its own outcome");
        let on = json!({ MODULE: true });
        task(&mut s, "person:carol", "turn", json!({ "thread": "th", "turn": true }), json!({}));
        task(&mut s, "chat:r_turn", "t1", json!({ "thread": "th", "turn": false, "from_run": "r_turn" }), on.clone());
        task(&mut s, "chat:r_turn", "old", json!({ "thread": "th", "turn": false, "from_run": "r_turn" }), json!({}));
        task(&mut s, "chat:r_turn", "stopped", json!({ "thread": "th", "turn": false, "from_run": "r_turn" }), on.clone());
        ev(&mut s, "kernel", json!({ "kind": "completed", "run": "r_turn", "summary": "ok" }));
        ev(&mut s, "kernel", json!({ "kind": "failed", "run": "r_t1", "reason": "the site was down", "by": "model" }));
        ev(&mut s, "kernel", json!({ "kind": "failed", "run": "r_old", "reason": "x", "by": "model" }));
        ev(&mut s, "person:carol", json!({ "kind": "cancelled", "run": "r_stopped" }));
        ev(&mut s, "kernel", json!({ "kind": "failed", "run": "r_stopped", "reason": "late", "by": "model" }));
        assert!(s.outcomes.is_due("t1"), "a failed task from chat does not return to its thread");
        assert!(!s.outcomes.is_due("turn") && !s.outcomes.is_due("old") && !s.outcomes.is_due("stopped"), "{:?}", s.outcomes.due);
        task(&mut s, "outcome:t1", "o1", json!({ "thread": "th", "turn": true }), json!({}));
        assert!(!s.outcomes.is_due("t1"), "the outcome returned and is still due");
        assert_eq!(s.outcomes.of("o1"), Some("t1"));
        // A second end of the same run changes nothing.
        ev(&mut s, "kernel", json!({ "kind": "failed", "run": "r_t1", "reason": "again", "by": "model" }));
        assert!(!s.outcomes.is_due("t1"), "an outcome returned twice");
    }

    /// A schedule's run with no conversation is due like any other, goes to
    /// its person as a notice, not a thread, and that notice returns it once.
    #[test]
    fn a_run_with_no_conversation_returns_as_one_notice() {
        let mut s = State::default();
        task(&mut s, "schedule:sc1", "run1", Value::Null, json!({ MODULE: true }));
        ev(&mut s, "kernel", json!({ "kind": "failed", "run": "r_run1", "reason": "no data", "by": "model" }));
        assert!(s.outcomes.is_due("run1"), "a failed scheduled run with no conversation reaches no one");
        assert!(notice_only(&s, &s.tasks["run1"]), "a run with no conversation would make a thread");
        let other = json!({ "kind": "item", "id": "i0", "item_kind": "notice", "task": "run1", "person": "carol", "summary": "x", "details": { "reason": "something_else" } });
        ev(&mut s, "kernel", other);
        assert!(s.outcomes.is_due("run1"), "another notice about the task returned its outcome");
        let notice = json!({ "kind": "item", "id": "i1", "item_kind": "notice", "task": "run1", "run": "r_run1", "person": "carol", "summary": "Needs your attention: x", "details": { "reason": NOTICE_REASON } });
        ev(&mut s, "kernel", notice);
        assert!(!s.outcomes.is_due("run1"), "the notice did not return the outcome, so it would be sent again");
        // A run that finished is not news: the schedule is done only when it stops for good.
        task(&mut s, "schedule:sc1", "run2", Value::Null, json!({ MODULE: true }));
        ev(&mut s, "kernel", json!({ "kind": "completed", "run": "r_run2", "summary": "Nothing new." }));
        assert!(!s.outcomes.is_due("run2"), "a heartbeat's every finished run sends a notice");
        // A finished task the person asked for, whose conversation is deleted, is news.
        task(&mut s, "chat:r_turn", "t9", json!({ "thread": "gone", "turn": false, "from_run": "r_turn" }), json!({ MODULE: true }));
        s.trash.mark_removed("gone", "carol");
        ev(&mut s, "kernel", json!({ "kind": "completed", "run": "r_t9", "summary": "ok" }));
        assert!(s.outcomes.is_due("t9"), "a finished task from a deleted conversation is never told");
    }

    /// A threadless routine's completed runs stay silent after setup (even
    /// the first, and even after deleting earlier runs); only a failed run
    /// or one waiting on the person wakes a notification.
    #[test]
    fn a_deleted_first_run_does_not_make_the_next_one_news() {
        let mut s = State::default();
        task(&mut s, "schedule:sc1", "task_hb1", Value::Null, json!({ MODULE: true }));
        ev(&mut s, "kernel", json!({ "kind": "completed", "run": "r_task_hb1", "summary": "Nothing new." }));
        assert!(!s.outcomes.is_due("task_hb1"), "a threadless routine's completed run should not send a lingering notice");
        ev(&mut s, "person:carol", json!({ "kind": "trashed", "item": "task_hb1", "person": "carol" }));
        assert!(!s.tasks.contains_key("task_hb1"), "the deleted run is still listed, so this test proves nothing");
        task(&mut s, "schedule:sc1", "task_hb2", Value::Null, json!({ MODULE: true }));
        let (t2, r2) = (s.tasks["task_hb2"].clone(), s.runs["r_task_hb2"].clone());
        assert!(routine(&s, &t2, &r2), "a later run at work is shown as news");
        ev(&mut s, "kernel", json!({ "kind": "completed", "run": "r_task_hb2", "summary": "Nothing new." }));
        assert!(!s.outcomes.is_due("task_hb2"), "deleting the first run made the next one send a notice");
        assert!(routine(&s, &s.tasks["task_hb2"].clone(), &s.runs["r_task_hb2"].clone()), "a later finished run is shown as news");
        task(&mut s, "schedule:sc1", "task_hb3", Value::Null, json!({ MODULE: true }));
        ev(&mut s, "kernel", json!({ "kind": "failed", "run": "r_task_hb3", "reason": "no data", "by": "model" }));
        assert!(!routine(&s, &s.tasks["task_hb3"].clone(), &s.runs["r_task_hb3"].clone()), "a failed run is hidden as routine");
        assert!(s.outcomes.is_due("task_hb3"), "a failed later run tells no one");
    }

    /// Heartbeat (a system task) stays completely silent on normal runs —
    /// even its first run, even if a chat thread were attached — and only
    /// surfaces when the person is needed (failed or waiting on a decision).
    #[test]
    fn system_heartbeat_stays_silent_unless_person_is_needed() {
        assert!(is_heartbeat_text(Some("Heartbeat"), "Check inbox"));
        assert!(is_heartbeat_text(None, "Heartbeat: Check connected email"));
        assert!(!is_heartbeat_text(Some("Daily digest"), "Summarize unread mail"));

        let mut s = State::default();
        let sys_mods = json!({ MODULE: true, SYSTEM_MODULE: true });
        task(&mut s, "schedule:hb", "hb1", json!({ "thread": "th", "turn": false }), sys_mods.clone());
        let (t1, r1) = (s.tasks["hb1"].clone(), s.runs["r_hb1"].clone());
        assert!(is_system_task(&s, &t1, &r1));
        assert!(routine(&s, &t1, &r1), "first running heartbeat must stay quiet (routine=true)");
        ev(&mut s, "kernel", json!({ "kind": "completed", "run": "r_hb1", "summary": "All quiet." }));
        assert!(!s.outcomes.is_due("hb1"), "completed heartbeat must never notify or wake chat");
        assert!(!result_shows(&s, &starters(&s), &s.tasks["hb1"]), "completed heartbeat must never post result to chat");
        assert!(routine(&s, &s.tasks["hb1"], &s.runs["r_hb1"]), "completed heartbeat must stay routine=true");

        // When a heartbeat run fails or needs the person, it DOES surface.
        task(&mut s, "schedule:hb", "hb2", Value::Null, sys_mods);
        ev(&mut s, "kernel", json!({ "kind": "failed", "run": "r_hb2", "reason": "needs sign-in", "by": "model" }));
        assert!(s.outcomes.is_due("hb2"), "failed heartbeat must notify the person");
        assert!(!routine(&s, &s.tasks["hb2"], &s.runs["r_hb2"]), "failed heartbeat must not be hidden as quiet routine");
    }

    /// A task whose conversation the person deleted is told by notice only.
    #[test]
    fn a_deleted_conversation_gets_a_notice_not_a_turn() {
        let mut s = State::default();
        task(&mut s, "chat:r_turn", "t1", json!({ "thread": "th", "turn": false, "from_run": "r_turn" }), json!({ MODULE: true }));
        assert!(!notice_only(&s, &s.tasks["t1"]));
        s.trash.mark_removed("th", "carol");
        assert!(notice_only(&s, &s.tasks["t1"]), "a task from a deleted conversation would post to it");
    }

    /// A thread shows a task on the card of the message that started it and
    /// its result once done; not a retry starting, the failed result a retry
    /// replaced, a scheduled run starting, nor the agent's handling of an
    /// outcome unless it stops to ask the person.
    #[test]
    fn a_thread_shows_only_what_needs_the_person_or_is_done() {
        let mut s = State::default();
        let on = json!({ MODULE: true });
        let (o1, o2) = (derived("task", "outcome:t1"), derived("task", "outcome:t2"));
        task(&mut s, "person:carol", "turn", json!({ "thread": "th", "turn": true }), json!({}));
        task(&mut s, "chat:r_turn", "t1", json!({ "thread": "th", "turn": false, "from_run": "r_turn" }), on.clone());
        task(&mut s, "schedule:sc1", "run1", json!({ "thread": "th", "turn": false }), on.clone());
        ev(&mut s, "kernel", json!({ "kind": "failed", "run": "r_t1", "reason": "x", "by": "model" }));
        let started = starters(&s);
        assert!(start_shows(&s, &s.tasks["t1"]), "the person's task is not on its message's card");
        assert!(!start_shows(&s, &s.tasks["run1"]), "a scheduled run posts that it started");
        assert!(result_shows(&s, &started, &s.tasks["t1"]), "a failed result with no retry is hidden");
        // The outcome turn retries: its reply and the retry starting are not shown, nor the result it replaces.
        task(&mut s, "outcome:t1", &o1, json!({ "thread": "th", "turn": true }), json!({}));
        let o1_run = s.tasks[&o1].run_id.clone();
        task(&mut s, &format!("chat:{o1_run}"), "t2", json!({ "thread": "th", "turn": false, "from_run": o1_run }), on.clone());
        ev(&mut s, "kernel", json!({ "kind": "completed", "run": o1_run, "summary": "Trying another way." }));
        let started = starters(&s);
        assert!(!reply_shows(&s, &started, "t1", &s.runs[&o1_run]), "the agent's retry note posts to the thread");
        assert!(!start_shows(&s, &s.tasks["t2"]), "a retry posts that it started");
        assert!(!result_shows(&s, &started, &s.tasks["t1"]), "the failed result a retry replaced still shows");
        // The retry fails too and the agent stops to ask: that reply shows.
        ev(&mut s, "kernel", json!({ "kind": "failed", "run": "r_t2", "reason": "x", "by": "model" }));
        task(&mut s, "outcome:t2", &o2, json!({ "thread": "th", "turn": true }), json!({}));
        let o2_run = s.tasks[&o2].run_id.clone();
        assert!(!reply_shows(&s, &started, "t2", &s.runs[&o2_run]), "a running outcome turn posts");
        ev(&mut s, "kernel", json!({ "kind": "completed", "run": o2_run, "summary": "It needs your sign-in." }));
        let started = starters(&s);
        assert!(reply_shows(&s, &started, "t2", &s.runs[&o2_run]), "the one decision the person must make is hidden");
        assert!(result_shows(&s, &started, &s.tasks["t2"]));
        // A repeating schedule shows the first time; a later run that
        // finished, in its conversation too, shows nothing and wakes no one.
        ev(&mut s, "kernel", json!({ "kind": "completed", "run": "r_run1", "summary": "ok" }));
        assert!(s.outcomes.is_due("run1") && result_shows(&s, &starters(&s), &s.tasks["run1"]), "a schedule's first run is not shown");
        task(&mut s, "schedule:sc1", "run2", json!({ "thread": "th", "turn": false }), on.clone());
        ev(&mut s, "kernel", json!({ "kind": "completed", "run": "r_run2", "summary": "ok" }));
        assert!(!s.outcomes.is_due("run2"), "a later finished scheduled run wakes the agent in its conversation");
        assert!(!result_shows(&s, &starters(&s), &s.tasks["run2"]), "a later finished scheduled run posts its result every time");
        task(&mut s, "schedule:sc1", "run3", json!({ "thread": "th", "turn": false }), on.clone());
        ev(&mut s, "kernel", json!({ "kind": "failed", "run": "r_run3", "reason": "needs a sign-in", "by": "model" }));
        assert!(s.outcomes.is_due("run3") && result_shows(&s, &starters(&s), &s.tasks["run3"]), "a later run that needs the person is hidden");
        // A finished task's result shows; its outcome turn never posts.
        task(&mut s, "chat:r_turn", "t3", json!({ "thread": "th", "turn": false, "from_run": "r_turn" }), on.clone());
        ev(&mut s, "kernel", json!({ "kind": "completed", "run": "r_t3", "summary": "ok" }));
        assert!(s.outcomes.is_due("t3") && result_shows(&s, &starters(&s), &s.tasks["t3"]), "a finished task is not told");
        let o3 = derived("task", "outcome:t3");
        task(&mut s, "outcome:t3", &o3, json!({ "thread": "th", "turn": true }), json!({}));
        let o3_run = s.tasks[&o3].run_id.clone();
        ev(&mut s, "kernel", json!({ "kind": "completed", "run": o3_run, "summary": "Done." }));
        assert!(!reply_shows(&s, &starters(&s), "t3", &s.runs[&o3_run]), "the agent restates a finished result");
    }

    /// Retries are counted back through the outcome turns that started
    /// them, and the turn's text says when the limit is reached.
    #[test]
    fn retries_count_back_through_outcome_turns() {
        let mut s = State::default();
        let on = json!({ MODULE: true });
        task(&mut s, "person:carol", "turn", json!({ "thread": "th", "turn": true }), json!({}));
        task(&mut s, "chat:r_turn", "t1", json!({ "thread": "th", "turn": false, "from_run": "r_turn" }), on.clone());
        assert_eq!(attempt(&s, "t1"), 0);
        ev(&mut s, "kernel", json!({ "kind": "failed", "run": "r_t1", "reason": "x", "by": "model" }));
        task(&mut s, "outcome:t1", "o1", json!({ "thread": "th", "turn": true }), json!({}));
        task(&mut s, "chat:r_o1", "t2", json!({ "thread": "th", "turn": false, "from_run": "r_o1" }), on.clone());
        assert_eq!(attempt(&s, "t2"), 1);
        ev(&mut s, "kernel", json!({ "kind": "failed", "run": "r_t2", "reason": "x", "by": "model" }));
        task(&mut s, "outcome:t2", "o2", json!({ "thread": "th", "turn": true }), json!({}));
        task(&mut s, "chat:r_o2", "t3", json!({ "thread": "th", "turn": false, "from_run": "r_o2" }), on);
        assert_eq!(attempt(&s, "t3"), 2);
        let left = turn_text("t1", Status::Failed, 0, 2);
        assert!(left.contains("retry 1 of 2") && left.contains("create_task") && left.contains("not a message from the person"), "{left}");
        let spent = turn_text("t3", Status::Failed, 2, 2);
        assert!(spent.contains("Retry limit reached (2 of 2)"), "{spent}");
        let done = turn_text("t3", Status::Completed, 2, 2);
        assert!(done.contains("finished") && done.contains("visible in the thread"), "{done}");
    }

    /// A retry is grouped under the person's own message, never under the
    /// outcome turn that started it: that turn's words are the server's.
    #[test]
    fn a_retry_belongs_to_the_turn_the_person_wrote() {
        let mut s = State::default();
        let on = json!({ MODULE: true });
        task(&mut s, "person:carol", "turn", json!({ "thread": "th", "turn": true }), json!({}));
        task(&mut s, "chat:r_turn", "t1", json!({ "thread": "th", "turn": false, "from_run": "r_turn" }), on.clone());
        ev(&mut s, "kernel", json!({ "kind": "failed", "run": "r_t1", "reason": "x", "by": "model" }));
        task(&mut s, "outcome:t1", "o1", json!({ "thread": "th", "turn": true }), json!({}));
        task(&mut s, "chat:r_o1", "t2", json!({ "thread": "th", "turn": false, "from_run": "r_o1" }), on.clone());
        ev(&mut s, "kernel", json!({ "kind": "failed", "run": "r_t2", "reason": "x", "by": "model" }));
        task(&mut s, "outcome:t2", "o2", json!({ "thread": "th", "turn": true }), json!({}));
        assert_eq!(person_turn(&s, &s.tasks["turn"]).task_id, "turn");
        assert_eq!(person_turn(&s, &s.tasks["o1"]).task_id, "turn", "a retry is grouped under the server's turn");
        assert_eq!(person_turn(&s, &s.tasks["o2"]).task_id, "turn", "a second retry is grouped under the server's turn");
    }

    /// The limit is validated when the configuration loads.
    #[test]
    fn the_retry_limit_is_bounded() {
        assert!(Settings::default().check().is_ok());
        assert_eq!(Settings::default().max_retry_count, DEFAULT_MAX_RETRY_COUNT);
        assert!(Settings { max_retry_count: MAX_RETRY_COUNT_CEILING + 1 }.check().is_err());
    }
}
