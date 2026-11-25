//! Where a schedule's runs post (owner, 2026-11-10: "Scheduled runs post in
//! the conversation where the schedule was set up, and a failed run wakes
//! the agent there"). A schedule confirmed on a chat card records that
//! conversation (`ScheduleSet.thread`). Each run posts its result there, and
//! its outcome returns there as a turn (outcomes.rs), with the retry limit.
//!
//! Work never makes a thread of its own (owner, 2026-11-16: "They should
//! not create a thread. They should appear in the thread that created them
//! only. If it's deleted they show a notification only"). A schedule set up
//! in settings or accepted from the inbox, one set up before the
//! conversation was recorded, and one whose conversation the person has
//! since trashed or removed post nowhere: each run's end reaches the person
//! as a notice (outcomes.rs).
use crate::state::{Schedule, State};

/// The conversation a schedule's runs post to, and where a run's outcome
/// wakes the agent: none for system schedules (e.g. Heartbeat) or when it
/// has no conversation that is still there.
pub fn of(s: &State, sc: &Schedule) -> Option<String> {
    if sc.system {
        return None;
    }
    sc.thread.as_ref().filter(|t| !s.trash.gone(t)).cloned()
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
    /// conversations were recorded, or outside one, makes no thread.
    #[test]
    fn a_schedule_posts_in_the_conversation_it_was_set_up_in() {
        let mut s = State::default();
        set(&mut s, 1, "sc_chat", json!("th_origin"));
        set(&mut s, 2, "sc_old", Value::Null);
        assert_eq!(of(&s, &s.arrangements.schedules()["sc_chat"]).as_deref(), Some("th_origin"), "a schedule set up in chat posts elsewhere");
        assert_eq!(of(&s, &s.arrangements.schedules()["sc_old"]), None, "a schedule with no conversation made a thread of its own");
    }

    /// A conversation the person removed is not written to again, and no
    /// thread replaces it.
    #[test]
    fn a_removed_conversation_leaves_the_runs_without_a_thread() {
        let mut s = State::default();
        set(&mut s, 1, "sc1", json!("th_gone"));
        s.trash.mark_removed("th_gone", "alice");
        assert_eq!(of(&s, &s.arrangements.schedules()["sc1"]), None);
    }

    /// A schedule its own run ended, with no end time or run limit, is
    /// never due again: it stayed due and fired again once that run was
    /// over, while its card said it had ended.
    #[test]
    fn a_schedule_its_run_ended_is_never_due_again() {
        let mut s = State::default();
        set(&mut s, 1, "sc1", Value::Null);
        assert!(s.arrangements.schedules()["sc1"].next_due().is_some());
        s.apply(&Event {
            seq: 2,
            at: chrono::Utc::now(),
            actor: "model".into(),
            ev: serde_json::from_value(json!({ "kind": "schedule_ended", "id": "sc1", "reason": "the work is done", "run": "r1" })).unwrap(),
        });
        assert!(s.arrangements.schedules()["sc1"].ended.is_some());
        assert!(s.arrangements.schedules()["sc1"].next_due().is_none(), "an ended schedule is still due");
    }

    /// A system schedule (e.g. Heartbeat) never posts into a chat conversation,
    /// even if a thread was supplied when it was set up.
    #[test]
    fn a_system_schedule_never_posts_in_chat() {
        let mut s = State::default();
        let ev = json!({ "kind": "schedule_set", "id": "hb1", "person": "alice", "text": "Heartbeat: Check inbox", "every_minutes": 30, "thread": "th_origin", "system": true });
        s.apply(&Event { seq: 1, at: chrono::Utc::now(), actor: "person:alice".into(), ev: serde_json::from_value(ev).unwrap() });
        let sc = &s.arrangements.schedules()["hb1"];
        assert!(sc.system, "heartbeat schedule must be marked system");
        assert_eq!(of(&s, sc), None, "system schedule must not post in chat");
    }
}
