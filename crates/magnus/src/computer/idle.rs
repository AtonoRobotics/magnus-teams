//! A computer no one is using hibernates to a snapshot.
use super::{Change, Role, Status};
use crate::events::Ev;
use crate::kernel::Kernel;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const IDLE_POLL: Duration = Duration::from_secs(60);
const SECONDS_PER_MINUTE: u64 = 60;

static LAST_USED: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
/// When each computer last began running, as this process has seen it.
static RUNNING_SINCE: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

/// The computers now over the run-time limit, given those running now and
/// not held by a person. Whatever stopped starts its count again.
fn over_run_limit(running: &[String], held: &[String], limit: Duration, now: Instant) -> Vec<String> {
    let mut slot = RUNNING_SINCE.lock().unwrap_or_else(|e| e.into_inner());
    let since = slot.get_or_insert_with(HashMap::new);
    since.retain(|id, _| running.contains(id));
    let mut over = vec![];
    for id in running {
        let began = *since.entry(id.clone()).or_insert(now);
        if now.saturating_duration_since(began) >= limit && !held.contains(id) {
            since.remove(id);
            over.push(id.clone());
        }
    }
    over
}

pub(super) fn touch(id: &str) {
    LAST_USED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(id.to_string(), Instant::now());
}

fn idle_for(id: &str) -> Duration {
    let mut slot = LAST_USED.lock().unwrap_or_else(|e| e.into_inner());
    slot.get_or_insert_with(HashMap::new).entry(id.to_string()).or_insert_with(Instant::now).elapsed()
}

/// Whether a browser VM has gone unused long enough to stop.
pub(super) fn idle_browser(id: &str, role: &Role, limit: Duration) -> bool {
    !matches!(role, Role::Computer) && idle_for(id) >= limit
}

pub(super) fn schedule(k: Arc<Kernel>) {
    let minutes = k.cfg.computer.as_ref().map(|c| c.idle_hibernate_minutes).unwrap_or(0);
    // Zero keeps computers running; the browsers are still tended.
    let limit = if minutes > 0 { Duration::from_secs(minutes * SECONDS_PER_MINUTE) } else { Duration::MAX };
    let run_limit = Duration::from_secs(k.cfg.computer.as_ref().map_or(0, |c| c.max_run_minutes) * SECONDS_PER_MINUTE);
    let Some(socket) = k.cfg.computer.as_ref().map(|c| c.socket.clone()) else { return };
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(IDLE_POLL);
        loop {
            tick.tick().await;
            k.tend_browsers(&socket).await;
            k.find_stopped_computers(&socket).await;
            let busy = |s: &crate::state::State, person: &str| s.runs.values().any(|r| r.person == person && r.status == crate::state::Status::Running);
            let candidates: Vec<String> = k.read(|s| {
                s.computers
                    .all()
                    .filter(|c| {
                        c.role == super::Role::Computer && c.status == Status::Running && c.controller.is_none() && !matches!(&c.owner, super::Owner::Person(p) if busy(s, p))
                    })
                    .map(|c| c.id.clone())
                    .collect()
            });
            let running: Vec<(String, bool)> =
                k.read(|s| s.computers.all().filter(|c| c.role == super::Role::Computer && c.status == Status::Running).map(|c| (c.id.clone(), c.controller.is_some())).collect());
            let (ids, held): (Vec<String>, Vec<String>) = (running.iter().map(|(i, _)| i.clone()).collect(), running.iter().filter(|(_, h)| *h).map(|(i, _)| i.clone()).collect());
            let over = if run_limit > Duration::ZERO { over_run_limit(&ids, &held, run_limit, Instant::now()) } else { vec![] };
            let due = candidates.into_iter().filter(|id| idle_for(id) >= limit && !over.contains(id)).chain(over.iter().cloned()).collect::<Vec<_>>();
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
                        && (over.contains(&c.id) || (idle_for(&c.id) >= limit && !matches!(&c.owner, super::Owner::Person(p) if busy(s, p))))
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
            let state = match super::client::ask(socket, serde_json::json!({ "op": "status", "id": id })).await {
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

    #[test]
    fn a_computer_over_its_run_time_stops_unless_a_person_holds_it() {
        let limit = Duration::from_secs(600);
        let start = Instant::now();
        let (a, b) = ("limit-a".to_string(), "limit-b".to_string());
        let both = [a.clone(), b.clone()];
        assert!(over_run_limit(&both, &[], limit, start).is_empty());
        let held = std::slice::from_ref(&b);
        assert_eq!(over_run_limit(&both, held, limit, start + limit), std::slice::from_ref(&a));
        // It stopped, so its count starts again.
        assert!(over_run_limit(&both, held, limit, start + limit).is_empty());
        assert_eq!(over_run_limit(&both, &[], limit, start + limit), std::slice::from_ref(&b));
    }
}
