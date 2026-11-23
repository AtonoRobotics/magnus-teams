//! What the routine sweep does with each due routine, decided from state
//! alone: the kernel only carries the steps out (`routines`). The same rules
//! as a person's schedule (kernel sweep): not while its last run is open,
//! and not once too many firings in a row failed.
use super::routines::{Firing, Routine};
use crate::state::{Run, State, Status};
use chrono::{DateTime, Utc};

/// What a due routine does now.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Its last run is still open: this firing starts nothing.
    Skip { task: String, why: String },
    /// Too many firings in a row failed: it starts no more runs until its
    /// admin changes or removes it.
    Stop { why: String },
    /// It starts its run.
    Fire,
}

/// A due routine and its step.
#[derive(Clone, Debug)]
pub struct Due {
    pub workspace: String,
    pub id: String,
    pub routine: Routine,
    pub step: Step,
}

/// The routines due at `now` and what each does. `stop_after_failures` is
/// the count that pauses a person's schedule; 0 never stops one. A routine
/// keeps only its latest firings, so one stops at that many at most.
pub fn plan(s: &State, now: DateTime<Utc>, stop_after_failures: u32) -> Vec<Due> {
    let kept = u32::try_from(super::routines::MAX_FIRINGS_KEPT).unwrap_or(u32::MAX);
    let stop_after_failures = stop_after_failures.min(kept);
    s.team
        .all()
        .flat_map(|w| {
            // Its author's own agent runs it (`start_room_mission_as`), so
            // that agent's holds decide: a held routine waits, unfired and
            // uncounted, as a person's schedule does (`due_schedules`).
            w.routines
                .iter()
                .filter(|(_, r)| r.stopped.is_none() && r.due(now) && w.members.contains_key(&r.by) && crate::halts::holds_new(s, &r.by, false).is_none())
                .map(|(id, r)| Due { workspace: w.id.clone(), id: id.clone(), routine: r.clone(), step: step(s, r, stop_after_failures) })
        })
        .collect()
}

fn step(s: &State, r: &Routine, n: u32) -> Step {
    if let Some((task, why)) = open_run(s, r) {
        return Step::Skip { task, why };
    }
    if n > 0 && failures_in_a_row(s, r) >= n {
        return Step::Stop { why: format!("its last {n} runs failed") };
    }
    Step::Fire
}

fn run_of<'a>(s: &'a State, task: &str) -> Option<&'a Run> {
    s.tasks.get(task).and_then(|t| s.runs.get(&t.run_id))
}

/// A run the routine started that has not ended, and why it is open.
fn open_run(s: &State, r: &Routine) -> Option<(String, String)> {
    r.fired.iter().rev().find_map(|f| {
        let Firing::Started(task) = f else { return None };
        let run = run_of(s, task).filter(|x| x.ended_at.is_none())?;
        let why = match (run.status == Status::Suspended, run.reason.as_deref()) {
            (true, Some("timer")) => "its last run is waiting for a time it set".to_string(),
            (true, Some(reason)) => format!("its last run is waiting on someone ({reason})"),
            _ => "its last run is still running".to_string(),
        };
        Some((task.clone(), why))
    })
}

/// How many of its latest firings in a row failed, since it was last
/// changed: firings that started nothing, and runs that ended failed.
fn failures_in_a_row(s: &State, r: &Routine) -> u32 {
    let mut n = 0;
    for f in r.fired.iter().rev() {
        match f {
            Firing::Failed => n += 1,
            Firing::Started(task) if run_of(s, task).is_some_and(|x| x.ended_at.is_some() && x.status == Status::Failed) => n += 1,
            Firing::Started(_) | Firing::Changed => break,
        }
    }
    n
}

/// A routine's name in a notice: its channel.
pub fn title(s: &State, workspace: &str, r: &Routine) -> String {
    let name = s.team.workspace(workspace).and_then(|w| w.channels.get(&r.channel)).map(|c| c.name.clone()).unwrap_or_default();
    format!("the routine in #{name}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Ev;
    use crate::log::Event;
    use crate::team::Change;
    use serde_json::json;

    const STOP_AFTER: u32 = 3;

    fn ev(s: &mut State, e: Ev) {
        let seq = s.last_seq + 1;
        s.apply(&Event { seq, at: Utc::now(), actor: "poll".into(), ev: e });
    }

    fn json_ev(s: &mut State, e: serde_json::Value) {
        ev(s, serde_json::from_value(e).unwrap());
    }

    /// A workspace `w` owned by ann with routine `r`, every hour.
    fn routine() -> State {
        let mut s = State::default();
        ev(&mut s, Ev::Team(Change::Created { workspace: "w".into(), name: "Launch".into(), owner: "ann".into() }));
        let r = Routine { channel: "c".into(), text: "Sum up the day.".into(), every_hours: 1, by: "ann".into(), ..Default::default() };
        ev(&mut s, Ev::Team(Change::Routine { workspace: "w".into(), id: "r".into(), routine: r }));
        s
    }

    fn ran(s: &mut State, task: &str, at: DateTime<Utc>) {
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "", "modules": {} });
        json_ev(s, json!({ "kind": "task_created", "task": task, "run": format!("run_{task}"), "person": "ann", "text": "x", "config": config }));
        ev(s, Ev::Team(Change::RoutineRan { workspace: "w".into(), id: "r".into(), task: task.into(), at }));
    }

    fn failed(s: &mut State, task: &str) {
        json_ev(s, json!({ "kind": "failed", "run": format!("run_{task}"), "reason": "the site was down", "by": "model" }));
    }

    fn could_not_start(s: &mut State, at: DateTime<Utc>) {
        ev(s, Ev::Team(Change::RoutineFailed { workspace: "w".into(), id: "r".into(), error: "no model".into(), at }));
    }

    fn steps(s: &State, now: DateTime<Utc>) -> Vec<Step> {
        plan(s, now, STOP_AFTER).into_iter().map(|d| d.step).collect()
    }

    /// A routine whose run outlasts its interval does not start a second
    /// one beside it: the firing is skipped, and it fires again once the
    /// run has ended.
    #[test]
    fn a_routine_does_not_fire_while_its_last_run_is_open() {
        let now = Utc::now();
        let mut s = routine();
        assert_eq!(steps(&s, now), vec![Step::Fire]);
        ran(&mut s, "t1", now - chrono::Duration::hours(2));
        let Some(Step::Skip { task, .. }) = steps(&s, now).pop() else { panic!("a routine fired beside its open run: {:?}", steps(&s, now)) };
        assert_eq!(task, "t1");
        ev(&mut s, Ev::Team(Change::RoutineSkipped { workspace: "w".into(), id: "r".into(), task, reason: "open".into(), at: now }));
        assert!(steps(&s, now).is_empty(), "a skipped firing is due again at once");
        json_ev(&mut s, json!({ "kind": "completed", "run": "run_t1", "summary": "ok" }));
        assert_eq!(steps(&s, now + chrono::Duration::hours(1)), vec![Step::Fire]);
    }

    /// Firings that cannot start, and runs that fail, count toward stopping
    /// it, as a schedule's do; a run that succeeds breaks the streak, and a
    /// stopped routine starts nothing until it is changed.
    #[test]
    fn a_routine_whose_firings_keep_failing_stops_until_changed() {
        let now = Utc::now();
        let mut s = routine();
        ran(&mut s, "t1", now - chrono::Duration::hours(5));
        failed(&mut s, "t1");
        could_not_start(&mut s, now - chrono::Duration::hours(4));
        assert_eq!(steps(&s, now), vec![Step::Fire], "two failures stopped it");
        ran(&mut s, "t2", now - chrono::Duration::hours(3));
        json_ev(&mut s, json!({ "kind": "completed", "run": "run_t2", "summary": "ok" }));
        could_not_start(&mut s, now - chrono::Duration::hours(2));
        could_not_start(&mut s, now - chrono::Duration::hours(1));
        assert_eq!(steps(&s, now), vec![Step::Fire], "a success did not break the streak");
        ran(&mut s, "t3", now - chrono::Duration::hours(1));
        failed(&mut s, "t3");
        let Some(Step::Stop { why }) = steps(&s, now).pop() else { panic!("a failing routine kept firing") };
        ev(&mut s, Ev::Team(Change::RoutineStopped { workspace: "w".into(), id: "r".into(), reason: why, at: now }));
        assert!(steps(&s, now + chrono::Duration::hours(9)).is_empty(), "a stopped routine is still due");
        let mut r = s.team.workspace("w").unwrap().routines["r"].clone();
        r.text = "Sum up the week.".into();
        ev(&mut s, Ev::Team(Change::Routine { workspace: "w".into(), id: "r".into(), routine: r }));
        assert_eq!(steps(&s, now), vec![Step::Fire], "a changed routine stayed stopped");
    }

    /// A routine keeps its latest firings only, however long it runs, and
    /// still stops: at the configured count, or at what it keeps when that
    /// is more. One changed keeps only its last run, which may be open.
    #[test]
    fn a_routine_keeps_few_firings_and_still_stops() {
        use super::super::routines::MAX_FIRINGS_KEPT;
        let now = Utc::now();
        let mut s = routine();
        let kept = |s: &State| s.team.workspace("w").unwrap().routines["r"].fired.clone();
        for n in 0..3 * MAX_FIRINGS_KEPT {
            let task = format!("t{n}");
            ran(&mut s, &task, now - chrono::Duration::hours(1));
            json_ev(&mut s, json!({ "kind": "completed", "run": format!("run_{task}"), "summary": "ok" }));
        }
        assert_eq!(kept(&s).len(), MAX_FIRINGS_KEPT, "every firing is kept");
        for _ in 0..MAX_FIRINGS_KEPT {
            could_not_start(&mut s, now - chrono::Duration::hours(1));
        }
        assert_eq!(kept(&s).len(), MAX_FIRINGS_KEPT);
        let many = u32::try_from(MAX_FIRINGS_KEPT).unwrap() * 2;
        assert!(matches!(plan(&s, now, many).pop().map(|d| d.step), Some(Step::Stop { .. })), "a count above what is kept never stops it");
        assert!(matches!(plan(&s, now, STOP_AFTER).pop().map(|d| d.step), Some(Step::Stop { .. })), "it no longer stops at the configured count");
        ran(&mut s, "open", now - chrono::Duration::hours(1));
        let mut r = s.team.workspace("w").unwrap().routines["r"].clone();
        r.text = "Sum up the week.".into();
        ev(&mut s, Ev::Team(Change::Routine { workspace: "w".into(), id: "r".into(), routine: r }));
        assert_eq!(kept(&s), vec![Firing::Started("open".into()), Firing::Changed], "a change kept what no longer counts");
        assert!(matches!(steps(&s, now).pop(), Some(Step::Skip { task, .. }) if task == "open"), "a change forgot its open run");
    }
}
