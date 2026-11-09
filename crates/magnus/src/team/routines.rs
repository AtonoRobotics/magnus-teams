//! The team agent's own work: on a schedule, under the workspace's standing
//! authority, reporting to a channel. It runs in the background and asks
//! no one, so anything it cannot do without asking does not happen.
use super::routes::{member, new_id, send, Reply, K};
use super::{Change, Role};
use crate::api::err;
use crate::events::Ev;
use crate::kernel::{Kernel, NewRun};
use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const ROUTINE_TICK: Duration = Duration::from_secs(60);
const MIN_ROUTINE_HOURS: i64 = 1;
const MAX_ROUTINE_HOURS: i64 = 24 * 30;
const MAX_ROUTINE_CHARS: usize = 4000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Routine {
    pub channel: String,
    pub text: String,
    pub every_hours: i64,
    /// The admin who set it. Its authority ends if they leave.
    pub by: String,
    #[serde(default)]
    pub last_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Routine {
    fn due(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        self.last_at.is_none_or(|at| now - at >= chrono::Duration::hours(self.every_hours))
    }
}

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/workspaces/{ws}/routines", post(add)).route("/v1/workspaces/{ws}/routines/{id}", delete(remove))
}

#[derive(Deserialize)]
struct NewRoutine {
    channel: String,
    text: String,
    every_hours: i64,
}

async fn add(State(k): K, h: HeaderMap, Path(ws): Path<String>, Json(b): Json<NewRoutine>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Admin).await);
    if !(MIN_ROUTINE_HOURS..=MAX_ROUTINE_HOURS).contains(&b.every_hours) {
        return err(StatusCode::BAD_REQUEST, format!("a routine runs every {MIN_ROUTINE_HOURS} to {MAX_ROUTINE_HOURS} hours"));
    }
    if b.text.trim().is_empty() || b.text.chars().count() > MAX_ROUTINE_CHARS {
        return err(StatusCode::BAD_REQUEST, "a routine needs instructions, and not too many");
    }
    if !k.read(|s| s.team.workspace(&ws).is_some_and(|w| w.channels.contains_key(&b.channel))) {
        return err(StatusCode::NOT_FOUND, "no such channel");
    }
    let id = new_id();
    let routine = Routine { channel: b.channel, text: b.text, every_hours: b.every_hours, by: p.id.clone(), last_at: None };
    ok_or_return!(send(&k, &p, Change::Routine { workspace: ws, id: id.clone(), routine }).await);
    if let Err(e) = k.run_due_routines().await {
        tracing::warn!("team routines: {e:#}");
    }
    Json(json!({ "id": id })).into_response()
}

async fn remove(State(k): K, h: HeaderMap, Path((ws, id)): Path<(String, String)>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Admin).await);
    ok_or_return!(send(&k, &p, Change::RoutineRemoved { workspace: ws, id }).await);
    Json(json!({ "ok": true })).into_response()
}

impl Kernel {
    pub(super) async fn run_due_routines(self: &Arc<Self>) -> Result<()> {
        let now = chrono::Utc::now();
        let due: Vec<(String, String, Routine)> = self.read(|s| {
            s.team.all().flat_map(|w| w.routines.iter().filter(|(_, r)| r.due(now) && w.members.contains_key(&r.by)).map(|(id, r)| (w.id.clone(), id.clone(), r.clone()))).collect()
        });
        for (workspace, id, r) in due {
            let (task, run) = self.create_run("poll", &r.by, r.text.clone(), vec![], NewRun::default()).await?;
            let change = Change::Run {
                workspace: workspace.clone(),
                channel: r.channel.clone(),
                thread: None,
                task: task.clone(),
                requester: r.by.clone(),
                history: String::new(),
                sub: false,
                outside: false,
            };
            self.emit("poll", Ev::Team(change)).await?;
            self.emit("poll", Ev::Team(Change::RoutineRan { workspace, id, task, at: now })).await?;
            self.wake(&run);
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
}
