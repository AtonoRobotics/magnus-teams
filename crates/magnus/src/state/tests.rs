use super::State;
use crate::log::Event;
use serde_json::json;

fn ev(seq: i64, actor: &str, ev: serde_json::Value) -> Event {
    Event { seq, at: chrono::Utc::now(), actor: actor.into(), ev: serde_json::from_value(ev).unwrap() }
}

/// A schedule's run counts against its limit as it is created, so a
/// crash before the firing is recorded never lets it run once more.
#[test]
fn a_scheduled_run_counts_when_it_is_created() {
    let mut s = State::default();
    s.apply(&ev(1, "person:alice", json!({ "kind": "schedule_set", "id": "s1", "person": "alice", "text": "Say hi.", "every_minutes": 60, "max_runs": 1 })));
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    s.apply(&ev(2, "schedule:s1", json!({ "kind": "task_created", "task": "t1", "run": "r1", "person": "alice", "text": "Say hi.", "config": config })));
    // The firing itself was lost: the schedule is still out of runs.
    assert_eq!(s.arrangements.schedules()["s1"].started, 1);
    assert!(s.arrangements.schedules()["s1"].next_due().is_none(), "a lost firing let the schedule run again");
    // And when the firing is recorded, it is not counted twice.
    s.apply(&ev(3, "kernel", json!({ "kind": "schedule_fired", "id": "s1", "task": "t1" })));
    assert_eq!(s.arrangements.schedules()["s1"].started, 1);
}

/// An admin's suspension marks the schedule until a resume clears it;
/// pauses written before suspensions existed replay unmarked (#262).
#[test]
fn an_admin_suspension_marks_the_schedule_until_resumed() {
    let mut s = State::default();
    s.apply(&ev(1, "person:alice", json!({ "kind": "schedule_set", "id": "s1", "person": "alice", "text": "Say hi.", "every_minutes": 60, "max_runs": 1 })));
    s.apply(&ev(2, "kernel", json!({ "kind": "schedule_paused", "id": "s1", "reason": "its last 3 runs failed" })));
    assert!(!s.arrangements.schedules()["s1"].paused_by_admin, "an old pause replayed as an admin's suspension");
    s.apply(&ev(3, "kernel", json!({ "kind": "schedule_resumed", "id": "s1" })));
    s.apply(&ev(4, "admin:root", json!({ "kind": "schedule_paused", "id": "s1", "reason": "Suspended by Root, an admin", "admin": true })));
    assert!(s.arrangements.schedules()["s1"].paused_by_admin, "the suspension left no mark");
    s.apply(&ev(5, "admin:root", json!({ "kind": "schedule_resumed", "id": "s1" })));
    assert!(!s.arrangements.schedules()["s1"].paused_by_admin, "the resume kept the mark");
}

/// A model call the provider charged for and then failed (a reply it
/// could not use) counts against the run's dollars and tokens.
#[test]
fn a_charged_call_that_failed_counts_against_the_budget() {
    let mut s = State::default();
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    s.apply(&ev(1, "person:alice", json!({ "kind": "task_created", "task": "t1", "run": "r1", "person": "alice", "text": "Say hi.", "config": config })));
    let usage = json!({ "input_tokens": 1000, "output_tokens": 200, "cost_usd": 0.25 });
    s.apply(&ev(2, "kernel", json!({ "kind": "model_error", "run": "r1", "step": 0, "attempt": 0, "class": "malformed", "message": "no model named", "usage": usage })));
    assert_eq!((s.runs["r1"].usd, s.runs["r1"].tokens.input, s.runs["r1"].tokens.output), (0.25, 1000, 200), "a charged failure was not counted");
    // An error recorded before usage was kept still loads, and costs nothing.
    s.apply(&ev(3, "kernel", json!({ "kind": "model_error", "run": "r1", "step": 0, "attempt": 1, "class": "unavailable", "message": "down" })));
    assert_eq!(s.runs["r1"].usd, 0.25);
}

/// Dismissing the notice a stopped run waits on leaves the run waiting on
/// no card, so the task's own Resume is not refused for it.
#[test]
fn a_dismissed_stop_notice_frees_the_run() {
    let mut s = State::default();
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    s.apply(&ev(1, "person:alice", json!({ "kind": "task_created", "task": "t1", "run": "r1", "person": "alice", "text": "Say hi.", "config": config })));
    s.apply(&ev(
        2,
        "kernel",
        json!({ "kind": "item", "id": "i1", "item_kind": "notice", "task": "t1", "run": "r1", "person": "alice", "summary": "Stopped", "details": { "reason": "budget" } }),
    ));
    s.apply(&ev(3, "kernel", json!({ "kind": "suspended", "run": "r1", "reason": "budget", "item": "i1" })));
    assert_eq!(s.runs["r1"].waiting_item.as_deref(), Some("i1"));
    s.apply(&ev(4, "person:alice", json!({ "kind": "item_resolved", "id": "i1", "decision": "dismiss" })));
    assert_eq!(s.runs["r1"].waiting_item, None, "the run still waits on a card put away");
    assert_eq!(s.runs["r1"].reason.as_deref(), Some("budget"), "dismissing it resumed nothing");
}

/// A run ends once: the first of completed, failed and cancelled stands.
/// A cancel that loses the race to the run's own end does not turn a
/// finished task into a cancelled one, nor its end into a cancel.
#[test]
fn a_run_ends_once_the_first_end_stands() {
    let mut s = State::default();
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    for (seq, t) in [(1, "1"), (2, "2")] {
        s.apply(&ev(
            seq,
            "person:alice",
            json!({ "kind": "task_created", "task": format!("t{t}"), "run": format!("r{t}"), "person": "alice", "text": "Say hi.", "config": config }),
        ));
    }
    s.apply(&ev(3, "kernel", json!({ "kind": "completed", "run": "r1", "summary": "Said hi." })));
    let ended = s.runs["r1"].ended_at;
    s.apply(&ev(4, "person:alice", json!({ "kind": "cancelled", "run": "r1" })));
    assert_eq!(s.runs["r1"].status, super::Status::Completed, "a cancel after the run completed made it cancelled");
    assert_eq!((s.runs["r1"].summary.as_deref(), s.runs["r1"].ended_at), (Some("Said hi."), ended));
    s.apply(&ev(5, "person:alice", json!({ "kind": "cancelled", "run": "r2" })));
    s.apply(&ev(6, "kernel", json!({ "kind": "failed", "run": "r2", "reason": "stopped", "by": "kernel" })));
    assert_eq!(s.runs["r2"].status, super::Status::Cancelled, "the run's own end after a cancel made it failed");
    assert_eq!(s.runs["r2"].summary, None);
}

/// A person erased leaves nothing of theirs in the state: not the
/// conversation a branch of theirs starts with, nor what their tasks
/// had attached; someone else's stay.
#[test]
fn a_person_erased_leaves_no_branch_or_attachment_of_theirs() {
    let mut s = State::default();
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    for (seq, p) in [(1, "bob"), (2, "alice")] {
        let file = json!([{ "id": format!("f-{p}"), "name": format!("{p}-salary.pdf") }]);
        s.apply(&ev(
            seq,
            &format!("person:{p}"),
            json!({ "kind": "task_created", "task": format!("t-{p}"), "run": format!("r-{p}"), "person": p, "text": "Read this.", "attachments": file, "config": config }),
        ));
        s.apply(&ev(seq + 10, &format!("person:{p}"), json!({ "kind": "thread_branched", "person": p, "thread": format!("b-{p}"), "from": format!("t-{p}"), "upto": "e1", "entries": [{ "id": "e1", "text": format!("{p} earns 91,000") }], "title": "Pay" })));
    }
    s.apply(&ev(30, "admin:root", json!({ "kind": "person_erased", "person": "bob" })));
    assert!(!s.thread_extras.branches().contains_key("b-bob"), "bob's branch kept his words");
    assert!(!s.attachments.contains_key("t-bob"), "bob's task kept what it had attached");
    assert!(s.thread_extras.branches().contains_key("b-alice") && s.attachments.contains_key("t-alice"), "alice's went with bob's");
}

/// A scoped draft records its threads and who is entitled to them, and
/// charges the run's call; erasing the person clears it (#178).
#[test]
fn a_scoped_draft_records_its_sources_and_charges_the_call() {
    let mut s = State::default();
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    s.apply(&ev(1, "person:alice", json!({ "kind": "task_created", "task": "t1", "run": "r1", "person": "alice", "text": "Answer it.", "config": config })));
    let usage = json!({ "input_tokens": 100, "output_tokens": 20, "cost_usd": 0.01 });
    s.apply(&ev(
        2,
        "kernel",
        json!({ "kind": "draft_scoped", "run": "r1", "person": "alice", "draft": "d1", "threads": ["tA"], "participants": ["bob@corp.example"], "usage": usage }),
    ));
    assert_eq!(s.draft_sources["d1"], super::DraftSources { person: "alice".into(), threads: vec!["tA".into()], participants: vec!["bob@corp.example".into()] });
    assert_eq!((s.runs["r1"].model_calls, s.runs["r1"].usd), (1, 0.01));
    s.apply(&ev(3, "admin:root", json!({ "kind": "person_erased", "person": "alice" })));
    assert!(!s.draft_sources.contains_key("d1"), "alice's draft kept its sources");
}

/// An ended schedule stays in state, marked with why, its pause
/// cleared: only a cancelled one is gone. It is not finished twice,
/// and an edit puts it back (#264).
#[test]
fn an_ended_schedule_stays_marked_with_why_until_cancelled() {
    let mut s = State::default();
    s.apply(&ev(1, "person:alice", json!({ "kind": "schedule_set", "id": "s1", "person": "alice", "text": "Say hi.", "every_minutes": 60, "max_runs": 1 })));
    s.apply(&ev(2, "person:alice", json!({ "kind": "schedule_paused", "id": "s1", "reason": "Paused by you" })));
    s.apply(&ev(3, "kernel", json!({ "kind": "schedule_ended", "id": "s1", "reason": "it ran 1 time, as set" })));
    let sc = s.arrangements.schedules().get("s1").expect("an ended schedule was removed");
    assert_eq!(sc.ended.as_deref(), Some("it ran 1 time, as set"), "the end kept no reason");
    assert!(sc.paused.is_none(), "the end kept the pause");
    assert!(sc.finished(chrono::Utc::now()).is_none(), "an ended schedule finishes again");
    // An edit puts it back: a new end, no mark.
    s.apply(&ev(4, "person:alice", json!({ "kind": "schedule_changed", "id": "s1", "every_minutes": 60, "daily_at": null, "until": null, "max_runs": 2 })));
    assert!(s.arrangements.schedules()["s1"].ended.is_none(), "the edit kept the end mark");
    // Cancelling removes it, as before.
    s.apply(&ev(5, "person:alice", json!({ "kind": "schedule_removed", "id": "s1" })));
    assert!(!s.arrangements.schedules().contains_key("s1"), "a cancelled schedule stayed");
}

/// Erasure drops watched baselines, home marks and opened marks
/// (#412): whoever is next known by the name starts fresh.
#[test]
fn erasure_drops_baselines_and_home_and_opened_marks() {
    let mut s = State::default();
    s.apply(&ev(1, "person:bob", json!({ "kind": "watched_seen", "person": "bob", "app": "mail", "tool": "list_inbox", "ids": ["a"], "full": true })));
    s.apply(&ev(2, "person:bob", json!({ "kind": "home_opened", "person": "bob" })));
    s.apply(&ev(3, "person:bob", json!({ "kind": "item", "id": "i1", "item_kind": "approval", "task": null, "run": null, "person": "bob" })));
    s.apply(&ev(4, "person:bob", json!({ "kind": "item_opened", "id": "i1" })));
    s.apply(&ev(5, "person:bob", json!({ "kind": "item_resolved", "id": "i1", "decision": "approved" })));
    assert!(!s.watch.is_empty() && s.identity.has_home_marks() && !s.inbox.items_opened().is_empty() && !s.inbox.items_resolved_at().is_empty(), "the setup left nothing to erase");
    s.apply(&ev(6, "kernel", json!({ "kind": "person_erased", "person": "bob" })));
    assert!(s.watch.is_empty(), "the erased baseline stayed: {:?}", s.watch);
    assert!(s.identity.home_last_open("bob").is_none(), "the erased home mark stayed");
    assert!(!s.inbox.items_opened().contains_key("i1"), "the erased opened mark stayed");
    assert!(!s.inbox.items_resolved_at().contains_key("i1"), "the erased resolved mark stayed");
}
