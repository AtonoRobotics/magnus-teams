//! The sweep's work on people's schedules: what to do is decided from state
//! alone (`ends`, `holds`, `firings`), and the kernel only carries the steps
//! out (`Kernel::sweep_schedules`). The phases are planned one after another,
//! each from the state the last one's steps left, as the sweep always did:
//! a schedule that ends is not held or fired, one held is not fired.
//!
//! A team's routines are a different thing (`team::routine_sweep`): only the
//! arithmetic of when they are due is shared (`cadence`).
use crate::events::Ev;
use crate::kernel::{Kernel, NewRun};
use crate::state::{Schedule, State};
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// One thing the sweep does with a schedule.
#[derive(Clone, Debug)]
pub enum Step {
    /// Past its end or out of runs: it is over, and its person is told.
    End { schedule: Schedule, why: String },
    /// Its run waits on its person: it pauses until they answer.
    Hold { schedule: Schedule, task: String, why: String },
    /// Held on a run that no longer waits: the hold lifts, if still so when
    /// the step is carried out. It goes on unless paused for another reason.
    Release { id: String },
    /// Due, but its last run is open: this firing starts nothing.
    Skip { schedule: Schedule, task: String, why: String },
    /// Due, but too many runs in a row failed: it pauses until its person looks.
    PauseFailures { schedule: Schedule, why: String },
    /// Due: it starts its run.
    Fire { schedule: Schedule },
}

/// Schedules past their end or out of runs.
pub fn ends(s: &State, now: DateTime<Utc>) -> Vec<Step> {
    s.arrangements.schedules().values().filter_map(|x| x.finished(now).map(|why| Step::End { schedule: x.clone(), why })).collect()
}

/// A schedule whose run waits on a person pauses until that person answers
/// (owner, 2026-10-30: without it, it acts without the authority or the
/// information it needs). The hold then lifts by itself; any other reason
/// it is paused for stays (`PauseCause`).
pub fn holds(s: &State) -> Vec<Step> {
    let mut hold = vec![];
    let mut release = vec![];
    for x in s.arrangements.schedules().values() {
        match (s.held_by_person(&x.id), x.held()) {
            (Some((task, why)), false) if x.paused.is_none() && x.ended.is_none() => hold.push(Step::Hold { schedule: x.clone(), task, why }),
            (None, true) => release.push(Step::Release { id: x.id.clone() }),
            _ => {}
        }
    }
    hold.extend(release);
    hold
}

/// The schedules due at `now`, and what each does. `pause_after_failures`
/// of 0 never pauses one.
pub fn firings(s: &State, now: DateTime<Utc>, pause_after_failures: u32) -> Vec<Step> {
    crate::halts::due_schedules(s, now).into_iter().map(|sc| firing(s, sc, pause_after_failures)).collect()
}

fn firing(s: &State, sc: Schedule, n: u32) -> Step {
    // Not while its last run is open: running, or waiting on someone. Runs
    // that pile up waiting flood the inbox (2026-10-30: 75 runs of one test
    // schedule).
    if let Some((task, why)) = s.open_run_of(&sc.id) {
        return Step::Skip { schedule: sc, task, why };
    }
    // Too many failures in a row: it stops spending until its person looks.
    if n > 0 && s.failures_in_a_row(&sc.id) >= n {
        return Step::PauseFailures { schedule: sc, why: format!("its last {n} runs failed") };
    }
    Step::Fire { schedule: sc }
}

impl Kernel {
    /// One sweep over people's schedules: each phase planned from the state
    /// the one before left, then carried out.
    pub(crate) async fn sweep_schedules(self: &Arc<Self>, now: DateTime<Utc>) {
        for step in self.read(|s| ends(s, now)) {
            self.carry_out_schedule_step(step).await;
        }
        for step in self.read(holds) {
            self.carry_out_schedule_step(step).await;
        }
        let n = self.cfg.schedules.pause_after_failures;
        for step in self.read(|s| firings(s, now, n)) {
            self.carry_out_schedule_step(step).await;
        }
    }

    async fn carry_out_schedule_step(self: &Arc<Self>, step: Step) {
        match step {
            Step::End { schedule: sc, why } => {
                let title = self.schedule_title_for_notice(&sc).await;
                let notice = self.notice_to(
                    None,
                    None,
                    sc.person.clone(),
                    format!("A schedule ended: {title}"),
                    serde_json::json!({ "reason": "schedule_ended", "evidence": { "schedule": sc.id, "why": why } }),
                );
                self.tell(vec![("kernel", Ev::ScheduleEnded { id: sc.id.clone(), reason: why, run: None }), ("kernel", notice)]).await;
            }
            Step::Hold { schedule: sc, task, why } => {
                let title = self.schedule_title_for_notice(&sc).await;
                let notice = self.notice_to(
                    Some(task.clone()),
                    None,
                    sc.person.clone(),
                    format!("A schedule paused until you answer its run: {title}"),
                    serde_json::json!({ "reason": "schedule_paused", "evidence": { "schedule": sc.id, "why": why, "task": task } }),
                );
                self.tell(vec![("kernel", Ev::SchedulePaused { id: sc.id.clone(), reason: why, task: Some(task), admin: false, source: None, turn: None }), ("kernel", notice)])
                    .await;
            }
            Step::Release { id } => {
                #[cfg(debug_assertions)]
                self.release_gate(&id).await;
                // The hold's end was read before the notifications above
                // went out. Only a hold on a run that no longer waits is
                // lifted (#318); the release lifts that cause alone, so a
                // pause that landed since stays.
                let ended = self.read(|s| s.held_by_person(&id).is_none());
                let held_pause = self.read(|s| s.arrangements.schedules().get(&id).is_some_and(|x| x.held()));
                if ended && held_pause {
                    let _ = self.emit("kernel", Ev::ScheduleResumed { id, source: None, turn: None }).await;
                }
            }
            Step::Skip { schedule: sc, task, why } => {
                let _ = self.emit("kernel", Ev::ScheduleSkipped { id: sc.id.clone(), task, reason: why }).await;
            }
            Step::PauseFailures { schedule: sc, why } => {
                let title = self.schedule_title_for_notice(&sc).await;
                let notice = self.notice_to(
                    None,
                    None,
                    sc.person.clone(),
                    format!("A schedule paused: {title}"),
                    serde_json::json!({ "reason": "schedule_paused", "evidence": { "schedule": sc.id, "why": why } }),
                );
                self.tell(vec![("kernel", Ev::SchedulePaused { id: sc.id.clone(), reason: why, task: None, admin: false, source: None, turn: None }), ("kernel", notice)]).await;
            }
            Step::Fire { schedule: sc } => self.fire_schedule(sc).await,
        }
    }

    async fn fire_schedule(self: &Arc<Self>, sc: Schedule) {
        // A scheduled task acts for the person who set it up (K54), within
        // the schedule's budget. Its runs start in the project the schedule
        // is in now. What it asks, and the scope its person confirmed,
        // recalled for them; a recall that fails is a firing that failed.
        let started = match self.words_of(&sc.person, sc.seq).await {
            Ok(w) => {
                #[cfg(debug_assertions)]
                self.sweep_gate(&sc.id).await;
                // Cancelled or paused while its words were recalled: no run
                // starts for a schedule that is gone or paused now (behavior
                // spec: cancelling stops future runs and any run in flight;
                // pausing stops future runs). No await stands between this
                // recheck and the creation below.
                if !self.read(|s| s.arrangements.schedules().get(&sc.id).is_some_and(|x| x.paused.is_none() && x.ended.is_none())) {
                    return;
                }
                let prompt_text = w.text(sc.seq).to_string();
                let o = NewRun {
                    // Unattended and reaching someone else: its untrusted reads
                    // are quarantined from its first step, recorded with it.
                    quarantine: self.schedule_quarantine(&sc, &prompt_text),
                    // The server's per-task limit, never one the model set on the card.
                    budget: Some(crate::task_budget::scheduled(&self.cfg.budget, sc.budget.as_ref())),
                    label: (sc.label != crate::tools::Label::PRINCIPAL).then_some(sc.label),
                    project: self.read(|s| s.project_of(&sc.id)),
                    scope: w.opt(sc.seq, "scope"),
                    // Each run posts in the conversation the schedule was set up in, where
                    // its outcome wakes the agent too (outcomes.rs, owner 2026-11-10). One
                    // with no conversation still there makes no thread (owner, 2026-11-16).
                    chat: self.read(|s| crate::schedule_thread::of(s, &sc)).map(|thread| crate::events::ChatOrigin {
                        thread,
                        turn: false,
                        from_run: None,
                        conversation: vec![],
                        opener: None,
                    }),
                    // A confirmed schedule runs on its own: the verifier judges the criteria
                    // no code checks, and only what it cannot judge goes to its person
                    // (owner, 2026-11-02; invariant 7 holds). Every run's end reaches its
                    // person: in its conversation, or as a notice when it has none.
                    modules: [("verifier".to_string(), true), (crate::outcomes::MODULE.to_string(), true)].into(),
                    ..Default::default()
                };
                self.create_run(&format!("schedule:{}", sc.id), &sc.person, prompt_text, vec![], o).await
            }
            Err(e) => Err(e),
        };
        match started {
            Ok((task, _)) => {
                let _ = self.emit("kernel", Ev::ScheduleFired { id: sc.id.clone(), task }).await;
            }
            Err(e) => {
                tracing::error!("schedule {}: {e:#}", sc.id);
                let title = self.schedule_title_for_notice(&sc).await;
                let notice = self.notice_to(
                    None,
                    None,
                    sc.person.clone(),
                    format!("A schedule could not start: {title}"),
                    serde_json::json!({ "reason": "schedule_failed", "evidence": { "schedule": sc.id, "error": format!("{e:#}") } }),
                );
                self.tell(vec![("kernel", Ev::ScheduleFailed { id: sc.id.clone(), reason: format!("{e:#}") }), ("kernel", notice)]).await;
            }
        }
    }

    /// Fires a schedule immediately at the person's explicit request ("Run now").
    pub(crate) async fn fire_schedule_now(self: &Arc<Self>, sc: Schedule) -> anyhow::Result<String> {
        let w = self.words_of(&sc.person, sc.seq).await?;
        let prompt_text = w.text(sc.seq).to_string();
        let o = NewRun {
            quarantine: self.schedule_quarantine(&sc, &prompt_text),
            budget: Some(crate::task_budget::scheduled(&self.cfg.budget, sc.budget.as_ref())),
            label: (sc.label != crate::tools::Label::PRINCIPAL).then_some(sc.label),
            project: self.read(|s| s.project_of(&sc.id)),
            scope: w.opt(sc.seq, "scope"),
            chat: self.read(|s| crate::schedule_thread::of(s, &sc)).map(|thread| crate::events::ChatOrigin {
                thread,
                turn: false,
                from_run: None,
                conversation: vec![],
                opener: None,
            }),
            modules: [("verifier".to_string(), true), (crate::outcomes::MODULE.to_string(), true)].into(),
            ..Default::default()
        };
        let (task, _) = self.create_run(&format!("schedule:{}", sc.id), &sc.person, prompt_text, vec![], o).await?;
        let _ = self.emit("kernel", Ev::ScheduleFired { id: sc.id.clone(), task: task.clone() }).await;
        Ok(task)
    }
}
