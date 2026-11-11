//! Admin › Agents, and the kill switch's levels below the server (spec rev
//! 141): every agent with what it runs, who asked and what it spent; pause
//! or stop one, a person's agents together, or the team agent; what an agent
//! did, from the log; and a switch on one person's agents, one agent or one
//! connector. Pressing a switch needs no step-up and releasing one does (the
//! guard's rule for `/v1/admin/kill-switch/.../press`); pausing and resuming
//! an agent are changes like any other. Each is in the audit log.
use super::mount::{audited, record, Audit};
use super::Admin;
use crate::halts::Scope;
use crate::kernel::Kernel;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) fn routes() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/admin/agents", get(list))
        .route("/v1/admin/agents/{agent}/activity", get(activity))
        .route("/v1/admin/agents/{agent}/pause", post(pause))
        .route("/v1/admin/agents/{agent}/resume", post(resume))
        .route("/v1/admin/kill-switches", get(switches))
        .route("/v1/admin/kill-switch/{level}/{id}/press", post(press))
        .route("/v1/admin/kill-switch/{level}/{id}/release", post(release))
}

fn refuse(status: StatusCode, v: Value) -> Response {
    (status, Json(v)).into_response()
}

fn answered(r: Result<Value, (StatusCode, Value)>) -> Response {
    match r {
        Ok(v) => Json(v).into_response(),
        Err((st, v)) => refuse(st, v),
    }
}

async fn list(State(k): State<Arc<Kernel>>) -> Response {
    Json(k.agents_view()).into_response()
}

/// Every switch below the server's that is on, every pause, and the server's.
async fn switches(State(k): State<Arc<Kernel>>) -> Response {
    Json(k.switches_all()).into_response()
}

/// What an agent did. A person's agent's is their data: the read is in the
/// log and they are told (the person-data rule).
async fn activity(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(agent): Path<String>) -> Response {
    match k.agent_activity(&a.actor(), &a.name(), &agent).await {
        Ok(v) => Json(v).into_response(),
        Err((st, why)) => refuse(st, json!({ "error": why })),
    }
}

async fn pause(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(agent): Path<String>) -> Response {
    paused(k, a, agent, true).await
}

async fn resume(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(agent): Path<String>) -> Response {
    paused(k, a, agent, false).await
}

async fn paused(k: Arc<Kernel>, a: Admin, agent: String, on: bool) -> Response {
    let audit = Audit {
        action: if on { "agent_paused" } else { "agent_resumed" }.into(),
        target: Some(agent.clone()),
        before: Some(json!({ "paused": !on })),
        after: Some(json!({ "paused": on })),
    };
    let change = async { answered(k.pause_agent(&a.actor(), &a.name(), &agent, on).await) };
    audited(&k, &a, audit, change).await
}

/// Stop it, at once and without a step-up. The switch records itself first,
/// so pressing works even when the audit entry cannot be written; the entry
/// follows, as the server level's does.
async fn press(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path((level, id)): Path<(String, String)>) -> Response {
    let Some(scope) = Scope::parse(&level, &id) else { return refuse(StatusCode::NOT_FOUND, json!({ "error": "a kill switch is on one person, one agent or one connector" })) };
    let v = match k.scoped_switch(&a.actor(), &a.name(), scope.clone(), true).await {
        Ok(v) => v,
        Err((st, v)) => return refuse(st, v),
    };
    let audit = Audit { action: "kill_switch_engaged".into(), target: Some(format!("{}:{}", scope.level(), scope.id())), before: None, after: Some(v.clone()) };
    if let Err(why) = record(&k, &a, &audit).await {
        let msg = format!("The kill switch is on, but its audit entry could not be written ({why}).");
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": "audit", "message": msg, "switch": v }));
    }
    Json(v).into_response()
}

/// Release it: after a step-up (the guard's), recorded first.
async fn release(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path((level, id)): Path<(String, String)>) -> Response {
    let Some(scope) = Scope::parse(&level, &id) else { return refuse(StatusCode::NOT_FOUND, json!({ "error": "a kill switch is on one person, one agent or one connector" })) };
    let audit = Audit {
        action: "kill_switch_released".into(),
        target: Some(format!("{}:{}", scope.level(), scope.id())),
        before: Some(json!({ "engaged": true })),
        after: Some(json!({ "engaged": false })),
    };
    let change = async { answered(k.scoped_switch(&a.actor(), &a.name(), scope.clone(), false).await) };
    audited(&k, &a, audit, change).await
}
