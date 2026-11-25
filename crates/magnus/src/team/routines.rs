//! A member's routine in a shared project: on its interval, their own
//! personal Magnus does the work in a mission lane and its scrubbed answer
//! reports to the routine's channel (2026-11-22-retire-team-agent-runs).
//! It runs in the background and asks no one, so anything it cannot do
//! without asking does not happen.
use super::caller::{at, Member};
use super::routes::{new_id, send, K};
use super::routine_sweep::Step;
use super::Change;
use crate::api_error::{refuse, ApiResult};
use crate::compartment::query_gate::scrub_check;
use crate::events::Ev;
use crate::kernel::Kernel;
use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const ROUTINE_TICK: Duration = Duration::from_secs(60);
const MIN_ROUTINE_HOURS: i64 = 1;
const MAX_ROUTINE_HOURS: i64 = 24 * 30;
/// A routine's instructions are its lane's objective, so they fit one.
const MAX_ROUTINE_CHARS: usize = super::orchestrator::MAX_OBJECTIVE_CHARS;
/// How many of a routine's latest firings are kept. The stop rule reads
/// back from the latest to the first that did not fail, so it counts at
/// most this many in a row (`routine_sweep` stops at this many when the
/// configured count is higher); older firings are forgotten.
pub(super) const MAX_FIRINGS_KEPT: usize = 32;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Routine {
    pub channel: String,
    pub text: String,
    pub every_hours: i64,
    /// The admin who set it. Its authority ends if they leave.
    pub by: String,
    #[serde(default)]
    pub last_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Its firings, oldest first, as the log records them: never written
    /// with the routine itself, which is the admin's setting.
    #[serde(skip)]
    pub fired: Vec<Firing>,
    /// Why it starts no more runs until it is changed or removed.
    #[serde(skip)]
    pub stopped: Option<String>,
}

/// One of a routine's firings, as `routine_sweep` counts them.
#[derive(Clone, Debug, PartialEq)]
pub enum Firing {
    /// It started this task.
    Started(String),
    /// It could not start a run.
    Failed,
    /// Its admin changed it: what came before no longer counts toward stopping it.
    Changed,
}

impl Routine {
    pub(super) fn due(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        let every = crate::cadence::Recurrence::EveryHours(self.every_hours);
        self.last_at.is_none_or(|at| every.next_after(Some(at), at, crate::cadence::Zone::Server).is_some_and(|t| t <= now))
    }
}

/// A routine change applied to its workspace. A firing, a failed one or a
/// skipped one, counts as the routine's last time, so it is next due one
/// interval on, as a schedule is.
pub(super) fn apply(w: &mut super::Workspace, c: &Change) {
    match c {
        Change::Routine { id, routine, .. } => {
            // What came before a change no longer counts toward stopping
            // it: only its last run is kept, which may still be open.
            let before = w.routines.remove(id).map(|old| old.fired).unwrap_or_default();
            let mut fired: Vec<Firing> = before.iter().rev().find(|f| matches!(f, Firing::Started(_))).cloned().into_iter().collect();
            if !before.is_empty() {
                fired.push(Firing::Changed);
            }
            w.routines.insert(id.clone(), Routine { fired, stopped: None, ..routine.clone() });
        }
        Change::RoutineRemoved { id, .. } => {
            w.routines.remove(id);
        }
        Change::RoutineRan { id, task, at, .. } => {
            if let Some(r) = w.routines.get_mut(id) {
                r.last_at = Some(*at);
                fire(r, Firing::Started(task.clone()));
            }
        }
        Change::RoutineFailed { id, at, .. } => {
            if let Some(r) = w.routines.get_mut(id) {
                r.last_at = Some(*at);
                fire(r, Firing::Failed);
            }
        }
        Change::RoutineSkipped { id, at, .. } => {
            if let Some(r) = w.routines.get_mut(id) {
                r.last_at = Some(*at);
            }
        }
        Change::RoutineStopped { id, reason, .. } => {
            if let Some(r) = w.routines.get_mut(id) {
                r.stopped = Some(reason.clone());
            }
        }
        Change::Removed { person, .. } => {
            for r in w.routines.values_mut().filter(|r| r.by == *person && r.stopped.is_none()) {
                r.stopped = Some("its creator left the workspace".to_string());
            }
        }
        Change::Member { person, role, .. } if *role < super::Role::Member => {
            for r in w.routines.values_mut().filter(|r| r.by == *person && r.stopped.is_none()) {
                r.stopped = Some("its creator is now a guest".to_string());
            }
        }
        _ => {}
    }
}

/// A firing kept, and the oldest forgotten past `MAX_FIRINGS_KEPT`. No run
/// it started is lost while open: a routine fires again only once its last
/// run has ended.
fn fire(r: &mut Routine, f: Firing) {
    r.fired.push(f);
    let over = r.fired.len().saturating_sub(MAX_FIRINGS_KEPT);
    r.fired.drain(..over);
}

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/workspaces/{ws}/routines", post(add)).route("/v1/workspaces/{ws}/routines/{id}", patch(update).delete(remove))
}

/// Why a routine's instructions cannot be its lane's objective, if so.
fn unfit(text: &str) -> Option<String> {
    if text.trim().is_empty() || text.chars().count() > MAX_ROUTINE_CHARS {
        return Some("a routine needs instructions, and not too many".to_string());
    }
    scrub_check(text).map(|why| format!("a routine's instructions are shared with the project: {why}"))
}

#[derive(Deserialize)]
struct NewRoutine {
    channel: String,
    text: String,
    every_hours: i64,
}

#[derive(Deserialize)]
struct UpdateRoutine {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    every_hours: Option<i64>,
}

async fn add(State(k): K, Member { person: p, .. }: Member<at::Member>, Path(ws): Path<String>, Json(b): Json<NewRoutine>) -> ApiResult {
    if !(MIN_ROUTINE_HOURS..=MAX_ROUTINE_HOURS).contains(&b.every_hours) {
        return refuse(StatusCode::BAD_REQUEST, format!("a routine runs every {MIN_ROUTINE_HOURS} to {MAX_ROUTINE_HOURS} hours"));
    }
    if let Some(why) = unfit(&b.text) {
        return refuse(StatusCode::BAD_REQUEST, why);
    }
    if !super::routes::has_channel(&k, &ws, &b.channel, &p.id) {
        return refuse(StatusCode::NOT_FOUND, "no such channel");
    }
    let id = new_id();
    let routine = Routine { channel: b.channel, text: b.text, every_hours: b.every_hours, by: p.id.clone(), ..Default::default() };
    send(&k, &p, Change::Routine { workspace: ws, id: id.clone(), routine }).await?;
    if let Err(e) = k.run_due_routines().await {
        tracing::warn!("team routines: {e:#}");
    }
    Ok(Json(json!({ "id": id })).into_response())
}

async fn update(State(k): K, Member { person: p, role, .. }: Member<at::Member>, Path((ws, id)): Path<(String, String)>, Json(b): Json<UpdateRoutine>) -> ApiResult {
    let Some(existing) = k.read(|s| s.team.workspace(&ws).and_then(|w| w.routines.get(&id).cloned())) else {
        return refuse(StatusCode::NOT_FOUND, "no such routine");
    };
    if existing.by != p.id && role < super::Role::Admin {
        return refuse(StatusCode::FORBIDDEN, "only who created this routine may change it");
    }
    let channel = b.channel.unwrap_or(existing.channel);
    let text = b.text.unwrap_or(existing.text);
    let every_hours = b.every_hours.unwrap_or(existing.every_hours);
    if !(MIN_ROUTINE_HOURS..=MAX_ROUTINE_HOURS).contains(&every_hours) {
        return refuse(StatusCode::BAD_REQUEST, format!("a routine runs every {MIN_ROUTINE_HOURS} to {MAX_ROUTINE_HOURS} hours"));
    }
    if let Some(why) = unfit(&text) {
        return refuse(StatusCode::BAD_REQUEST, why);
    }
    if !super::routes::has_channel(&k, &ws, &channel, &p.id) {
        return refuse(StatusCode::NOT_FOUND, "no such channel");
    }
    let routine = Routine { channel, text, every_hours, by: p.id.clone(), last_at: existing.last_at, ..Default::default() };
    send(&k, &p, Change::Routine { workspace: ws, id, routine }).await?;
    if let Err(e) = k.run_due_routines().await {
        tracing::warn!("team routines: {e:#}");
    }
    Ok(Json(json!({ "ok": true })).into_response())
}

async fn remove(State(k): K, Member { person: p, role, .. }: Member<at::Member>, Path((ws, id)): Path<(String, String)>) -> ApiResult {
    let existing = k.read(|s| s.team.workspace(&ws).and_then(|w| w.routines.get(&id).cloned()));
    if let Some(r) = existing {
        if r.by != p.id && role < super::Role::Admin {
            return refuse(StatusCode::FORBIDDEN, "only who created this routine may remove it");
        }
    }
    send(&k, &p, Change::RoutineRemoved { workspace: ws, id }).await?;
    Ok(Json(json!({ "ok": true })).into_response())
}

impl Kernel {
    /// Starts the routines that are due. A routine that cannot start is
    /// recorded and its admin told, and the rest still start; one whose last
    /// run is open starts nothing; one whose firings keep failing stops
    /// (`routine_sweep`).
    pub(super) async fn run_due_routines(self: &Arc<Self>) -> Result<()> {
        let now = chrono::Utc::now();
        let n = self.cfg.schedules.pause_after_failures;
        let due = self.read(|s| super::routine_sweep::plan(s, now, n));
        for d in due {
            let (workspace, id, r) = (d.workspace, d.id, d.routine);
            let title = self.read(|s| super::routine_sweep::title(s, &workspace, &r));
            match d.step {
                Step::Skip { task, why } => {
                    self.emit("poll", Ev::Team(Change::RoutineSkipped { workspace, id, task, reason: why, at: now })).await?;
                }
                Step::Stop { why } => {
                    let notice = self.notice_to(
                        None,
                        None,
                        r.by.clone(),
                        format!("A routine stopped: {title}"),
                        json!({ "reason": "routine_stopped", "evidence": { "workspace": workspace, "routine": id, "why": why } }),
                    );
                    self.emit_all(vec![("poll", Ev::Team(Change::RoutineStopped { workspace, id, reason: why, at: now })), ("poll", notice)]).await?;
                }
                Step::Fire => {
                    // Its author's own agent does it, in the background,
                    // and its answer reports to the channel unasked: work
                    // nobody watches that reaches others, so the one rule
                    // for unattended runs quarantines its untrusted reads.
                    let text = r.text.trim();
                    let quarantine = self.unattended_quarantine(&crate::quarantine_runtime::Unattended { id: &id, person: &r.by, text, posts_to_others: true });
                    match self.start_room_mission_as("poll", &workspace, &r.channel, &r.by, text, None, quarantine, false, Some(id.clone())).await {
                        Ok((_, task, _)) => {
                            self.emit("poll", Ev::Team(Change::RoutineRan { workspace, id, task, at: now })).await?;
                        }
                        Err(e) => {
                            tracing::error!("team routine {id}: {e:#}");
                            let error = format!("{e:#}");
                            let notice = self.notice_to(
                                None,
                                None,
                                r.by.clone(),
                                format!("A routine could not start: {title}"),
                                json!({ "reason": "routine_failed", "evidence": { "workspace": workspace, "routine": id, "error": error } }),
                            );
                            self.emit_all(vec![("poll", Ev::Team(Change::RoutineFailed { workspace, id, error, at: now })), ("poll", notice)]).await?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// Checks each minute for routines that are due.
pub fn schedule(k: Arc<Kernel>) {
    tokio::spawn(async move {
        let mut t = tokio::time::interval(ROUTINE_TICK);
        loop {
            t.tick().await;
            if let Err(e) = k.run_due_routines().await {
                tracing::warn!("team routines: {e:#}");
            }
            if let Err(e) = k.follow_up_quiet_threads().await {
                tracing::warn!("team follow-ups: {e:#}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_routine_is_due_first_at_once_and_then_on_its_interval() {
        let now = chrono::Utc::now();
        let mut r = Routine { every_hours: 2, ..Default::default() };
        assert!(r.due(now));
        r.last_at = Some(now - chrono::Duration::hours(1));
        assert!(!r.due(now));
        r.last_at = Some(now - chrono::Duration::hours(2));
        assert!(r.due(now));
    }

    #[test]
    fn a_routine_stops_when_its_creator_leaves_or_becomes_guest() {
        let mut w = super::super::Workspace::default();
        let r = Routine { channel: "c".into(), text: "Sum up".into(), every_hours: 1, by: "ann".into(), ..Default::default() };
        apply(&mut w, &Change::Routine { workspace: "w".into(), id: "r1".into(), routine: r.clone() });
        assert!(w.routines["r1"].stopped.is_none());
        // Peers (Member) keep their routines; only demotion to Guest stops them.
        apply(&mut w, &Change::Member { workspace: "w".into(), person: "ann".into(), role: super::super::Role::Member });
        assert!(w.routines["r1"].stopped.is_none(), "a peer member lost their own routine");
        apply(&mut w, &Change::Member { workspace: "w".into(), person: "ann".into(), role: super::super::Role::Guest });
        assert!(w.routines["r1"].stopped.is_some(), "demoting creator to Guest did not stop their routine");

        apply(&mut w, &Change::Routine { workspace: "w".into(), id: "r2".into(), routine: r });
        assert!(w.routines["r2"].stopped.is_none());
        apply(&mut w, &Change::Removed { workspace: "w".into(), person: "ann".into() });
        assert!(w.routines["r2"].stopped.is_some(), "removing creator did not stop their routine");
    }
}
