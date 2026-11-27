//! A computer no one is using hibernates to a snapshot.
use super::client::Request;
use super::{Change, Computer, Ledger, Role, Status};
use crate::events::Ev;
use crate::kernel::Kernel;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const IDLE_POLL_SECS: Duration = Duration::from_secs(60);
const SECONDS_PER_MINUTE: u64 = 60;

/// When each computer was last used, as this process has seen it. The log's
/// own record of it counts too, so a restart resets no clock.
static LAST_USED: Mutex<Option<HashMap<String, DateTime<Utc>>>> = Mutex::new(None);

/// The log's time on a computer: a start begins its run, and anything that
/// leaves it not running ends it.
pub(super) fn keep_time(c: &mut Computer, change: &Change, at: DateTime<Utc>) {
    if matches!(change, Change::Started { .. }) {
        c.running_since = Some(at);
    } else if c.status != Status::Running {
        c.running_since = None;
    }
}

fn elapsed(since: DateTime<Utc>, now: DateTime<Utc>) -> Duration {
    (now - since).to_std().unwrap_or_default()
}

/// The computers over the run-time limit: running since the log last
/// recorded them starting, and not held by a person.
pub(super) fn over_run_limit(ledger: &Ledger, limit: Duration, now: DateTime<Utc>) -> Vec<String> {
    ledger
        .all()
        .filter(|c| c.role == Role::Computer && c.status == Status::Running && c.controller.is_none())
        .filter(|c| c.running_since.is_some_and(|t| elapsed(t, now) >= limit))
        .map(|c| c.id.clone())
        .collect()
}

pub(super) fn touch(id: &str) {
    LAST_USED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(id.to_string(), Utc::now());
}

/// How long `c` has gone unused: since its last use here or the log's last
/// record of it, whichever is later.
pub(super) fn idle_for(c: &Computer, now: DateTime<Utc>) -> Duration {
    let touched = LAST_USED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).get(&c.id).copied();
    touched.max(c.last_used).map_or(Duration::ZERO, |t| elapsed(t, now))
}

/// A limit configured in minutes. A number too large to count in seconds
/// is as good as no limit: it must never wrap to a short one, which would
/// hibernate or stop every computer at once.
fn minutes_limit(minutes: u64) -> Duration {
    Duration::from_secs(minutes.saturating_mul(SECONDS_PER_MINUTE))
}

/// Whether a browser VM has gone unused long enough to stop.
pub(super) fn idle_browser(c: &Computer, limit: Duration) -> bool {
    !matches!(c.role, Role::Computer) && idle_for(c, Utc::now()) >= limit
}

pub(super) fn schedule(k: Arc<Kernel>) {
    let minutes = k.cfg.computer.as_ref().map(|c| c.idle_hibernate_minutes).unwrap_or(0);
    // Zero keeps computers running; the browsers are still tended.
    let limit = if minutes > 0 { minutes_limit(minutes) } else { Duration::MAX };
    let run_limit = minutes_limit(k.cfg.computer.as_ref().map_or(0, |c| c.max_run_minutes));
    let Some(socket) = k.cfg.computer.as_ref().map(|c| c.socket.clone()) else { return };
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(IDLE_POLL_SECS);
        loop {
            tick.tick().await;
            k.tend_browsers(&socket).await;
            k.find_stopped_computers(&socket).await;
            let busy = |s: &crate::state::State, person: &str| s.runs.values().any(|r| r.person == person && r.status == crate::state::Status::Running);
            let now = Utc::now();
            let idle = |s: &crate::state::State, c: &Computer| {
                c.role == Role::Computer
                    && c.status == Status::Running
                    && c.controller.is_none()
                    && !matches!(&c.owner, super::Owner::Person(p) if busy(s, p))
                    && idle_for(c, now) >= limit
            };
            let candidates: Vec<String> = k.read(|s| s.computers.all().filter(|c| idle(s, c)).map(|c| c.id.clone()).collect());
            let over = if run_limit > Duration::ZERO { k.read(|s| over_run_limit(&s.computers, run_limit, now)) } else { vec![] };
            let due = candidates.into_iter().filter(|id| !over.contains(id)).chain(over.iter().cloned()).collect::<Vec<_>>();
            for id in due {
                // No wake may start while it goes to sleep, and none may be
                // under way: hibernating then would snapshot the VM a wake
                // just booted, and the wake would fail.
                let Some(_held) = super::wake::claim_hibernation(&id) else { continue };
                // The list is as old as the hibernations ahead of this one,
                // which take minutes on a slow disk: it must still be due.
                let still_due = |s: &crate::state::State, c: &super::Computer| {
                    c.status == Status::Running
                        && c.controller.is_none()
                        && (over.contains(&c.id) || (idle_for(c, Utc::now()) >= limit && !matches!(&c.owner, super::Owner::Person(p) if busy(s, p))))
                };
                let Some(c) = k.read(|s| s.computers.get(&id).filter(|c| still_due(s, c)).cloned()) else { continue };
                match k.zero_and_hibernate(&socket, &c).await {
                    Ok(()) => {
                        if let Err(e) = k.emit("kernel", Ev::Computer(Change::Hibernated { id: id.clone() })).await {
                            tracing::warn!("computer {id} hibernated but could not be recorded: {e:#}");
                        }
                    }
                    Err(e) => tracing::warn!("computer {id} could not hibernate: {e:#}"),
                }
            }
        }
    });
}

/// What the machine's answer about a computer Magnus has running means: it
/// still runs (or is paused), or it stopped, kept asleep when a snapshot was.
pub(super) fn found_stopped(state: &str) -> Option<bool> {
    match state {
        "stopped" => Some(false),
        "hibernated" => Some(true),
        _ => None,
    }
}

impl Kernel {
    /// A computer Magnus has running that the machine no longer runs is
    /// recorded stopped, so opening its screen wakes it rather than failing.
    async fn find_stopped_computers(&self, socket: &std::path::Path) {
        let running: Vec<String> = self.read(|s| s.computers.all().filter(|c| c.role == Role::Computer && c.status == Status::Running).map(|c| c.id.clone()).collect());
        for id in running {
            // Not while a wake boots it or a hibernation puts it to sleep.
            let Some(_held) = super::wake::claim_hibernation(&id) else { continue };
            let state = match super::client::ask(socket, Request::Status { id: id.to_string() }).await {
                Ok(v) => v["state"].as_str().unwrap_or_default().to_string(),
                Err(e) => {
                    tracing::warn!("computer {id}: could not ask the machine whether it runs: {e:#}");
                    continue;
                }
            };
            let Some(hibernated) = found_stopped(&state) else { continue };
            if !self.read(|s| s.computers.get(&id).is_some_and(|c| c.status == Status::Running)) {
                continue;
            }
            tracing::warn!("computer {id} was not running on the machine; recorded as {state}");
            if let Err(e) = self.emit("kernel", Ev::Computer(Change::FoundStopped { id: id.clone(), hibernated })).await {
                tracing::warn!("computer {id}: could not record that it stopped: {e:#}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_huge_minutes_setting_is_no_limit_not_a_short_one() {
        assert_eq!(minutes_limit(u64::MAX), Duration::from_secs(u64::MAX));
        assert_eq!(minutes_limit(u64::MAX / 30), Duration::from_secs(u64::MAX), "the limit wrapped");
        assert_eq!(minutes_limit(15), Duration::from_secs(15 * 60));
    }

    /// A computer the machine stopped is recorded stopped, or asleep when it
    /// kept a snapshot; one still running or paused is left alone.
    #[test]
    fn the_machines_answer_decides_whether_a_computer_stopped() {
        assert_eq!(found_stopped("stopped"), Some(false));
        assert_eq!(found_stopped("hibernated"), Some(true));
        for live in ["running", "paused", ""] {
            assert_eq!(found_stopped(live), None, "{live}");
        }
        let mut s = super::super::Ledger::default();
        s.apply(&Change::Created { id: "a".into(), owner: super::super::Owner::Person("p".into()) });
        s.apply(&Change::Started { id: "a".into() });
        s.apply(&Change::FoundStopped { id: "a".into(), hibernated: false });
        assert_eq!(s.get("a").map(|c| c.status), Some(Status::Stopped), "a stopped computer still reads as running, so its screen fails");
        s.apply(&Change::Started { id: "a".into() });
        s.apply(&Change::FoundStopped { id: "a".into(), hibernated: true });
        assert_eq!(s.get("a").map(|c| c.status), Some(Status::Hibernated));
    }

    fn person(p: &str) -> super::super::Owner {
        super::super::Owner::Person(p.into())
    }

    /// The limit counts from the start the log recorded, so a restart of
    /// Magnus in between resets nothing; a person holding it keeps it up.
    #[test]
    fn the_run_limit_counts_from_the_logged_start_and_survives_a_restart() {
        let limit = Duration::from_secs(24 * 60 * 60);
        let now = Utc::now();
        let began = now - chrono::Duration::hours(25);
        // A ledger rebuilt by replay, as after a restart.
        let mut s = Ledger::default();
        for id in ["a", "b", "c"] {
            s.apply_at(&Change::Created { id: id.into(), owner: person(id) }, began);
            s.apply_at(&Change::Started { id: id.into() }, began);
        }
        s.apply_at(&Change::Started { id: "c".into() }, now - chrono::Duration::hours(1));
        s.apply_at(&Change::TakeoverStarted { id: "b".into(), by: "b".into(), at: now, cause: None, runs: vec![] }, now);
        assert_eq!(over_run_limit(&s, limit, now), ["a"]);
        s.apply_at(&Change::Hibernated { id: "a".into() }, now);
        s.apply_at(&Change::Started { id: "a".into() }, now);
        assert!(over_run_limit(&s, limit, now).is_empty(), "a computer started again kept its old count");
    }

    /// Idle counts from the later of a use seen here and the log's last
    /// record, so a restart does not give every computer a fresh idle spell.
    #[test]
    fn the_idle_clock_counts_from_the_log_after_a_restart() {
        let now = Utc::now();
        let mut s = Ledger::default();
        s.apply_at(&Change::Created { id: "idle-a".into(), owner: person("p") }, now - chrono::Duration::hours(2));
        let c = s.get("idle-a").cloned().unwrap();
        assert!(idle_for(&c, now) >= Duration::from_secs(2 * 60 * 60 - 1), "the idle clock started again at the restart");
        touch("idle-a");
        assert!(idle_for(&c, Utc::now()) < Duration::from_secs(60));
    }
}
