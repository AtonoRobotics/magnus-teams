//! Per-person Server-Sent Events invalidation stream (`GET /v1/events`).
//!
//! Instead of every browser tab polling 12 endpoints every 10 seconds, the
//! kernel broadcasts compact `{ seq, domains }` frames whenever an event is
//! appended. Clients re-fetch only the invalidated slice(s).

use super::{who, work::async_stream, K};
use crate::api_error::ApiResult;
use crate::events::Ev;
use crate::extract::Caller;
use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue},
    response::IntoResponse,
};
use serde_json::json;
use std::time::Duration;

/// Capacity of the kernel's in-memory SSE invalidation broadcast ring.
pub(crate) const INVALIDATION_CHANNEL_CAPACITY: usize = 256;
/// How often an open `/v1/events` stream re-verifies the caller's session and sends a keepalive.
const EVENTS_RECHECK_SECS: Duration = Duration::from_secs(15);

/// All domains invalidated on ring lag or global state changes.
const ALL_DOMAINS: &[&str] = &["tasks", "inbox", "chat", "schedules", "connectors", "projects", "computers", "workspaces", "staged"];

/// One invalidation frame pushed to signed-in UI clients.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Invalidation {
    pub seq: i64,
    /// `Some(person_id)` when the event belongs to one person; `None` when broadcast-relevant.
    pub person: Option<String>,
    pub domains: &'static [&'static str],
}

/// Map a committed log event to the UI polling slice(s) it invalidates.
pub(crate) fn invalidation_domains(ev: &Ev) -> &'static [&'static str] {
    match ev {
        Ev::TaskCreated { .. }
        | Ev::Completed { .. }
        | Ev::Failed { .. }
        | Ev::Cancelled { .. }
        | Ev::Suspended { .. }
        | Ev::Resumed { .. }
        | Ev::Plan { .. }
        | Ev::ModelReply(_)
        | Ev::ToolResult { .. }
        | Ev::Output(_)
        | Ev::Verdict(_)
        | Ev::BudgetExtended { .. }
        | Ev::Wait { .. } => &["tasks", "chat"],
        Ev::Item { .. } | Ev::ItemResolved { .. } | Ev::ItemOpened { .. } | Ev::Approval(_) | Ev::ApprovalUsed { .. } => &["inbox", "tasks"],
        Ev::ThreadBranched(_) | Ev::Reacted(_) | Ev::CardDeclined(_) => &["chat"],
        Ev::ScheduleSet { .. } | Ev::SchedulePaused(_) | Ev::ScheduleResumed(_) | Ev::ScheduleChanged(_) | Ev::ScheduleEnded(_) | Ev::ScheduleRemoved(_) => &["schedules", "tasks"],
        Ev::ConnectorSettings(_) | Ev::ToolPinned(_) | Ev::ToolChanged(_) | Ev::Catalog(_) | Ev::AccountLink(_) => &["connectors"],
        Ev::ProjectSet(_) | Ev::Moved { .. } => &["projects"],
        Ev::Computer(_) | Ev::LocalComputer(_) => &["computers"],
        Ev::Team(_) | Ev::TeamMission(_) => &["workspaces"],
        Ev::Staged(_) => &["staged"],
        Ev::KillSwitch { .. } | Ev::Trashed { .. } | Ev::Restored { .. } | Ev::Removed { .. } => &["tasks", "inbox", "chat", "schedules", "projects"],
        _ => &[],
    }
}

/// Whether a signed-in person should receive this invalidation frame.
pub(crate) fn visible_to(inv: &Invalidation, person_id: &str) -> bool {
    !inv.domains.is_empty() && inv.person.as_deref().is_none_or(|p| p == person_id)
}

/// `GET /v1/events`: per-person SSE invalidation stream.
pub(super) async fn events_stream(State(k): K, Caller(p): Caller, h: HeaderMap) -> ApiResult {
    let mut rx = k.invalidations.subscribe();
    let initial_seq = k.read(|s| s.last_seq);
    let stream = async_stream(move |tx| async move {
        if tx.send(format!("data: {}\n\n", json!({ "seq": initial_seq, "domains": [] }))).await.is_err() {
            return;
        }
        let mut tick = tokio::time::interval(EVENTS_RECHECK_SECS);
        loop {
            tokio::select! {
                () = tx.closed() => return,
                _ = tick.tick() => {
                    if !who(&k, &h).await.is_ok_and(|q| q.id == p.id) {
                        return;
                    }
                    if tx.send(": keepalive\n\n".to_string()).await.is_err() {
                        return;
                    }
                }
                got = rx.recv() => match got {
                    Ok(inv) if visible_to(&inv, &p.id) => {
                        let line = format!("data: {}\n\n", json!({ "seq": inv.seq, "domains": inv.domains }));
                        if tx.send(line).await.is_err() {
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let seq = k.read(|s| s.last_seq);
                        let line = format!("data: {}\n\n", json!({ "seq": seq, "domains": ALL_DOMAINS }));
                        if tx.send(line).await.is_err() {
                            return;
                        }
                    }
                    Err(_) => return,
                },
            }
        }
    });
    let mut r = axum::body::Body::from_stream(stream).into_response();
    r.headers_mut().insert("content-type", HeaderValue::from_static("text/event-stream"));
    r.headers_mut().insert("cache-control", HeaderValue::from_static("no-cache"));
    r.headers_mut().insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains_map_key_events_and_filter_by_person() {
        assert_eq!(invalidation_domains(&Ev::Completed { run: "r1".into(), summary: "ok".into(), view: None, spoken: None }), &["tasks", "chat"]);
        assert_eq!(invalidation_domains(&Ev::ItemOpened { id: "i1".into() }), &["inbox", "tasks"]);
        assert_eq!(invalidation_domains(&Ev::KillSwitch { engaged: true }), &["tasks", "inbox", "chat", "schedules", "projects"]);
        assert_eq!(invalidation_domains(&Ev::Moved { item: "t1".into(), project: None }), &["projects"]);
        assert!(invalidation_domains(&Ev::SecretSet { name: "k".into() }).is_empty());

        let mine = Invalidation { seq: 10, person: Some("alice".into()), domains: &["tasks"] };
        let other = Invalidation { seq: 11, person: Some("bob".into()), domains: &["tasks"] };
        let global = Invalidation { seq: 12, person: None, domains: &["connectors"] };
        assert!(visible_to(&mine, "alice"));
        assert!(!visible_to(&other, "alice"));
        assert!(visible_to(&global, "alice"));
    }
}
