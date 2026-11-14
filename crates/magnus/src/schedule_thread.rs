//! Where a schedule's runs post (owner, 2026-11-10: "Scheduled runs post in
//! the conversation where the schedule was set up, and a failed run wakes
//! the agent there"). A schedule confirmed on a chat card records that
//! conversation (`ScheduleSet.thread`). Each run posts its result there, and
//! its outcome returns there as a turn (outcomes.rs), with the retry limit.
//!
//! A schedule with no conversation keeps a thread of its own,
//! `schedule-{id}`: one set up in settings or accepted from the inbox, one
//! set up before the conversation was recorded, and one whose conversation
//! the person has since trashed or removed.
use crate::state::{Schedule, State};

/// The thread a schedule's runs post to, and where a run's outcome wakes
/// the agent.
pub fn of(s: &State, sc: &Schedule) -> String {
    let here = |t: &&String| !s.trash.contains_key(*t) && !s.removed_threads.contains(*t);
    sc.thread.as_ref().filter(here).cloned().unwrap_or_else(|| crate::chat::schedule_thread(&sc.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::Event;
    use serde_json::{json, Value};

    fn set(s: &mut State, seq: i64, id: &str, thread: Value) {
        let mut ev = json!({ "kind": "schedule_set", "id": id, "person": "alice", "text": "Summarise my inbox.", "daily_at": "08:00" });
        if !thread.is_null() {
            ev["thread"] = thread;
        }
        s.apply(&Event { seq, at: chrono::Utc::now(), actor: "person:alice".into(), ev: serde_json::from_value(ev).unwrap() });
    }

    /// A schedule set up in a conversation posts there; one set up before
    /// conversations were recorded, or outside one, posts in its own thread.
    #[test]
    fn a_schedule_posts_in_the_conversation_it_was_set_up_in() {
        let mut s = State::default();
        set(&mut s, 1, "sc_chat", json!("th_origin"));
        set(&mut s, 2, "sc_old", Value::Null);
        assert_eq!(of(&s, &s.schedules["sc_chat"]), "th_origin", "a schedule set up in chat posts elsewhere");
        assert_eq!(of(&s, &s.schedules["sc_old"]), "schedule-sc_old", "a schedule with no conversation lost its own thread");
    }

    /// A conversation the person removed is not written to again: the
    /// schedule's runs go to its own thread.
    #[test]
    fn a_removed_conversation_sends_the_runs_to_the_schedules_own_thread() {
        let mut s = State::default();
        set(&mut s, 1, "sc1", json!("th_gone"));
        s.removed_threads.insert("th_gone".into());
        assert_eq!(of(&s, &s.schedules["sc1"]), "schedule-sc1");
    }

    /// A schedule its own run ended, with no end time or run limit, is
    /// never due again: it stayed due and fired again once that run was
    /// over, while its card said it had ended.
    #[test]
    fn a_schedule_its_run_ended_is_never_due_again() {
        let mut s = State::default();
        set(&mut s, 1, "sc1", Value::Null);
        assert!(s.schedules["sc1"].next_due().is_some());
        s.apply(&Event {
            seq: 2,
            at: chrono::Utc::now(),
            actor: "model".into(),
            ev: serde_json::from_value(json!({ "kind": "schedule_ended", "id": "sc1", "reason": "the work is done", "run": "r1" })).unwrap(),
        });
        assert!(s.schedules["sc1"].ended.is_some());
        assert!(s.schedules["sc1"].next_due().is_none(), "an ended schedule is still due");
    }
}
