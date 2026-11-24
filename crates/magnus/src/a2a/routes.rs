//! The outside agents API. A member's, beside their apps: list, connect by
//! card URL, admit, disconnect. And the outside agents' own, reached
//! without a paired device (`PUBLIC_PATHS`): the server's signed card, its
//! signing keys, and the one JSON-RPC endpoint, which authenticates each
//! request itself.

use super::card::{self, CARD_PATH, ENDPOINT_PATH, KEYS_PATH, KEY_HEADER};
use super::connect::{view, Connect};
use super::ledger::Target;
use super::MAX_REQUEST_BYTES;
use crate::api::{err, internal};
use crate::extract::Caller;
use crate::kernel::Kernel;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

type K = State<Arc<Kernel>>;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/outside-agents", get(list).post(connect))
        .route("/v1/outside-agents/{id}", delete(disconnect))
        .route("/v1/outside-agents/{id}/admit", post(admit))
        .route(CARD_PATH, get(agent_card))
        .route(KEYS_PATH, get(card_keys))
        .route(ENDPOINT_PATH, post(endpoint).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)))
}

fn refused((st, m): crate::connectors::Refusal) -> Response {
    err(st, m)
}

async fn list(State(k): K, Caller(p): Caller) -> Response {
    let agents: Vec<Value> = k.read(|s| s.outside_agents.of(&p.id).into_iter().map(|a| view(a, s.outside_agents.admissions(&a.id))).collect());
    Json(json!({ "agents": agents })).into_response()
}

async fn connect(State(k): K, Caller(p): Caller, Json(b): Json<Connect>) -> Response {
    match k.connect_outside_agent(&p.id, b).await {
        Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
        Err(r) => refused(r),
    }
}

/// `{"to": {"person": "<id>"}}` or `{"to": {"project": "<id>"}}`.
async fn admit(State(k): K, Caller(p): Caller, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let Ok(to) = serde_json::from_value::<Target>(b["to"].clone()) else {
        return err(StatusCode::BAD_REQUEST, "to is {\"person\": \"<id>\"} or {\"project\": \"<id>\"}");
    };
    match k.admit_outside_agent(&p.id, &id, to).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => refused(r),
    }
}

async fn disconnect(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> Response {
    match k.disconnect_outside_agent(&p.id, &id).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => refused(r),
    }
}

/// The server's card, signed with this period's key: only the skills some
/// admitted agent may use. Published once the server has a public address.
async fn agent_card(State(k): K) -> Response {
    let Some(base) = k.cfg.appliance.public_url.clone() else {
        return err(StatusCode::NOT_FOUND, "this server has no public address set, so it publishes no agent card");
    };
    let (person, project) = k.read(|s| s.outside_agents.admitted_kinds());
    let ours = card::ours(&base.join(ENDPOINT_PATH), person, project);
    match super::keys::signed(&k.vault, ours, &base.join(KEYS_PATH), chrono::Utc::now()) {
        Ok(c) => Json(c).into_response(),
        Err(e) => internal(e),
    }
}

async fn card_keys(State(k): K) -> Response {
    match super::keys::key_set(&k.vault, chrono::Utc::now()) {
        Ok(set) => Json(set).into_response(),
        Err(e) => internal(e),
    }
}

/// The JSON-RPC endpoint: never a session's, only an outside agent's key.
async fn endpoint(State(k): K, headers: HeaderMap, body: Bytes) -> Response {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let (st, answer) = k.outside_request(header(KEY_HEADER).as_deref(), header("a2a-version").as_deref(), &body).await;
    (st, Json(answer)).into_response()
}
