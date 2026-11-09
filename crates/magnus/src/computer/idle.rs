//! A computer no one is using hibernates to a snapshot.
use super::{client, Change, Role, Status};
use crate::events::Ev;
use crate::kernel::Kernel;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const IDLE_POLL: Duration = Duration::from_secs(60);
const SECONDS_PER_MINUTE: u64 = 60;

static LAST_USED: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

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
    let Some(socket) = k.cfg.computer.as_ref().map(|c| c.socket.clone()) else { return };
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(IDLE_POLL);
        loop {
            tick.tick().await;
            k.tend_browsers(&socket).await;
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
            for id in candidates.into_iter().filter(|id| idle_for(id) >= limit) {
                match client::ask(&socket, serde_json::json!({ "op": "hibernate", "id": id })).await {
                    Ok(_) => {
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
