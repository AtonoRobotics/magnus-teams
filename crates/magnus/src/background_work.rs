//! What Magnus's own background work (a review, a watch poll) waits for,
//! and what wakes it (spec rev 142, Permissions and tasks and Proactive,
//! helpful autonomy).
//!
//! "On each event Magnus decides for itself whether to suggest or to act
//! ... It asks only when Auto Review says ask." An ask from background work
//! goes to its person like any other, and the work waits on it as unwatched
//! work does (`unattended`): 30 minutes, then a message stays a draft and
//! the run carries on with what does not depend on the ask. The team agent's
//! own routines are the exception: they run under the workspace's standing
//! authority and report to the channel, with no person of their own to ask.

use crate::state::{State, Status};

impl State {
    /// Whether background work of this run may put an ask in front of its
    /// person: all of it but the team agent's own routines (and their
    /// sub-agents), which have no person of their own.
    pub fn background_asks(&self, run: &str) -> bool {
        let task = self.runs.get(run).map(|r| r.task.as_str()).unwrap_or_default();
        self.team.run_of(task).is_none()
    }

    /// Whether a background run stopping for `reason` waits for its person
    /// rather than ending: only on an ask, and only where it may ask.
    pub fn background_waits(&self, run: &str, reason: &str) -> bool {
        reason == "approval" && self.background_asks(run)
    }

    /// Whether a review of this person's is going that holds the next one.
    /// A review waiting on an ask holds nothing: what does not depend on the
    /// ask goes on, new events included.
    pub fn reviewing(&self, person: &str) -> bool {
        self.review_runs
            .iter()
            .filter(|(_, p)| p.as_str() == person)
            .any(|(run, _)| !self.runs.get(run).is_some_and(|r| r.status == Status::Suspended && r.reason.as_deref() == Some("approval")))
    }

    /// Whether this ended run's outcome comes back to the agent as a turn in
    /// its conversation (`outcomes`), or to its parent as a sub-agent's
    /// result. Such an end wakes no review as well: the agent already has it,
    /// and one end makes one run.
    pub fn outcome_turn_follows(&self, run: &str) -> bool {
        if self.sub_agents.get(run).is_some() {
            return true;
        }
        let Some(r) = self.runs.get(run).filter(|r| r.config.module(crate::outcomes::MODULE)) else { return false };
        let thread = self.tasks.get(&r.task).and_then(|t| t.chat.as_ref()).filter(|c| !c.turn).map(|c| c.thread.as_str());
        thread.is_some_and(|th| !self.trash.contains_key(th) && !self.removed_threads.contains(th))
    }
}

#[cfg(test)]
mod tests {
    use crate::log::Event;
    use crate::state::State;
    use serde_json::{json, Value};

    fn ev(s: &mut State, actor: &str, e: Value) {
        let seq = s.last_seq + 1;
        s.apply(&Event { seq, at: chrono::Utc::now(), actor: actor.into(), ev: serde_json::from_value(e).unwrap() });
    }

    fn task(s: &mut State, actor: &str, task: &str, chat: Value, modules: Value) {
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 7, "max_actions": 9, "max_usd": 3.0 }, "verifier": false, "instructions": "", "harness": "", "modules": modules });
        ev(s, actor, json!({ "kind": "task_created", "task": task, "run": format!("r_{task}"), "person": "carol", "text": "x", "config": config, "chat": chat }));
    }

    /// A review's ask waits for its person; any other stop of background
    /// work still ends it.
    #[test]
    fn a_review_waits_on_its_ask_and_on_nothing_else() {
        let mut s = State::default();
        task(&mut s, "review", "rv", Value::Null, json!({}));
        assert!(s.tasks["rv"].background());
        assert!(s.background_asks("r_rv"), "a review cannot ask its person");
        assert!(s.background_waits("r_rv", "approval"), "a review's ask ended the review");
        assert!(!s.background_waits("r_rv", "question"), "a review waits on a question");
        assert!(!s.background_waits("r_rv", "budget"), "a review waits on its budget");
    }

    /// A review waiting on an ask holds no later review; a running one does.
    #[test]
    fn a_review_waiting_on_an_ask_holds_no_later_review() {
        let mut s = State::default();
        task(&mut s, "review", "rv", Value::Null, json!({}));
        ev(&mut s, "kernel", json!({ "kind": "review_woken", "run": "r_rv", "task": "rv", "person": "carol", "reason": "cadence" }));
        assert!(s.reviewing("carol"), "a running review does not hold the next");
        ev(&mut s, "kernel", json!({ "kind": "suspended", "run": "r_rv", "reason": "approval", "item": "item_1" }));
        assert!(!s.reviewing("carol"), "a review waiting on its ask held every later review");
        assert!(!s.reviewing("dan"));
    }

    /// A task whose outcome returns to its conversation wakes no review; a
    /// task from no conversation, or from a deleted one, still does.
    #[test]
    fn an_outcome_that_returns_as_a_turn_is_not_reviewed_too() {
        let mut s = State::default();
        let on = json!({ crate::outcomes::MODULE: true });
        task(&mut s, "person:carol", "turn", json!({ "thread": "th", "turn": true }), json!({}));
        task(&mut s, "chat:r_turn", "t1", json!({ "thread": "th", "turn": false, "from_run": "r_turn" }), on.clone());
        task(&mut s, "schedule:sc1", "run1", json!({ "thread": "schedule-sc1", "turn": false }), on);
        task(&mut s, "person:carol", "plain", Value::Null, json!({}));
        assert!(s.outcome_turn_follows("r_t1"), "a task from chat would also wake a review");
        assert!(s.outcome_turn_follows("r_run1"), "a scheduled run would also wake a review");
        assert!(!s.outcome_turn_follows("r_plain"), "a task from no conversation wakes nothing");
        assert!(!s.outcome_turn_follows("r_turn"), "a chat turn returns no outcome");
    }
}
