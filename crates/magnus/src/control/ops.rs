//! Control routes for agent policy, run modules, model providers and updates.

use super::{actor, text};
use crate::extract::Body;
use crate::kernel::Kernel;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

fn policy_refused((st, m): crate::policy::admin::Refusal) -> Response {
    (st, Json(json!({ "error": m }))).into_response()
}

/// The sets, and what a test can name: the appliance's people and the
/// organisation's tools.
pub(super) async fn policy_sets(State(k): State<Arc<Kernel>>) -> Response {
    let people: Vec<Value> = k.people().all().into_iter().map(|p| json!({ "id": p.id, "name": p.name, "groups": p.groups })).collect();
    let tools: Vec<Value> = k
        .connectors()
        .into_iter()
        .flat_map(|c| c.tools.iter().map(|(t, cl)| json!({ "name": format!("{}__{t}", c.name), "risk": cl.risk, "audience": c.audience })).collect::<Vec<_>>())
        .collect();
    Json(json!({ "sets": k.policy_sets(), "people": people, "tools": tools })).into_response()
}

pub(super) async fn policy_draft(State(k): State<Arc<Kernel>>, Body(b): Body<Value>) -> Response {
    let (Some(actor), Some(layer)) = (actor(&b), text(&b, "layer")) else { return policy_refused((StatusCode::BAD_REQUEST, "name who drafts which layer".into())) };
    match k.policy_draft(&actor, &layer, b["cedar"].as_str().map(String::from)).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => policy_refused(r),
    }
}

pub(super) async fn policy_apply(State(k): State<Arc<Kernel>>, Body(b): Body<Value>) -> Response {
    let (Some(actor), Some(layer)) = (actor(&b), text(&b, "layer")) else { return policy_refused((StatusCode::BAD_REQUEST, "name who applies which layer".into())) };
    match k.policy_apply(&actor, &layer, b["cedar"].as_str().unwrap_or_default()).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => policy_refused(r),
    }
}

pub(super) async fn policy_test(State(k): State<Arc<Kernel>>, Body(b): Body<Value>) -> Response {
    match k.policy_test(&b) {
        Ok(v) => Json(v).into_response(),
        Err(r) => policy_refused(r),
    }
}

pub(super) async fn policy_history(State(k): State<Arc<Kernel>>, Query(q): Query<HashMap<String, String>>) -> Response {
    Json(json!({ "versions": k.policy_history(q.get("layer").map(String::as_str).unwrap_or("org")) })).into_response()
}

fn ops_refused((st, m): crate::ops_admin::Refusal) -> Response {
    (st, Json(json!({ "error": m }))).into_response()
}

fn ops(r: Result<Value, crate::ops_admin::Refusal>) -> Response {
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => ops_refused(e),
    }
}

pub(super) async fn modules(State(k): State<Arc<Kernel>>) -> Response {
    Json(json!({ "modules": k.admin_modules() })).into_response()
}

pub(super) async fn set_module(State(k): State<Arc<Kernel>>, Path(name): Path<String>, Body(b): Body<Value>) -> Response {
    let Some(actor) = actor(&b) else { return ops_refused((StatusCode::BAD_REQUEST, "name who sets it".into())) };
    ops(k.set_module(&actor, &name, b["on"].as_bool()).await)
}

pub(super) async fn providers(State(k): State<Arc<Kernel>>) -> Response {
    Json(json!({ "providers": k.admin_providers().await })).into_response()
}

pub(super) async fn test_provider(State(k): State<Arc<Kernel>>, Path(id): Path<String>) -> Response {
    ops(k.test_provider(&id).await)
}

pub(super) async fn set_provider(State(k): State<Arc<Kernel>>, Path(id): Path<String>, Body(b): Body<Value>) -> Response {
    let Some(actor) = actor(&b) else { return ops_refused((StatusCode::BAD_REQUEST, "name who sets it".into())) };
    ops(k.set_provider(&actor, &id, b["enabled"].as_bool(), b["default_model"].as_str()).await)
}

pub(super) async fn updates(State(k): State<Arc<Kernel>>) -> Response {
    Json(k.admin_updates()).into_response()
}

/// Install the latest release now; the switch happens after the answer.
pub(super) async fn install_update(State(k): State<Arc<Kernel>>, Body(b): Body<Value>) -> Response {
    let Some(actor) = actor(&b) else { return ops_refused((StatusCode::BAD_REQUEST, "name who installs it".into())) };
    let current = crate::update::version();
    let to = k.latest_release().and_then(|m| m["version"].as_str().map(String::from));
    match to {
        Some(to) if crate::update::newer(&to, &current) && !k.read(|s| s.server_ops.skipped(&to)) => {
            tokio::spawn(async move {
                if let Err(e) = k.update(&actor).await {
                    tracing::error!("update: {e:#}");
                }
            });
            (StatusCode::ACCEPTED, Json(json!({ "status": "installing", "version": current, "latest": to }))).into_response()
        }
        to => Json(json!({ "status": "up_to_date", "version": current, "latest": to })).into_response(),
    }
}

/// Go back to the release kept; the switch happens after the answer.
pub(super) async fn roll_back_update(State(k): State<Arc<Kernel>>, Body(b): Body<Value>) -> Response {
    let Some(actor) = actor(&b) else { return ops_refused((StatusCode::BAD_REQUEST, "name who rolls it back".into())) };
    let Some(to) = crate::update::previous_version() else { return ops_refused((StatusCode::CONFLICT, "no earlier release is kept on this server".into())) };
    tokio::spawn(async move {
        if let Err(e) = k.roll_back(&actor).await {
            tracing::error!("roll back: {e:#}");
        }
    });
    (StatusCode::ACCEPTED, Json(json!({ "status": "rolling_back", "version": crate::update::version(), "to": to }))).into_response()
}

pub(super) async fn desktop_channel(State(k): State<Arc<Kernel>>, Body(b): Body<Value>) -> Response {
    let Some(actor) = actor(&b) else { return ops_refused((StatusCode::BAD_REQUEST, "name who sets it".into())) };
    ops(k.set_desktop_channel(&actor, b["channel"].as_str().unwrap_or_default()).await)
}
