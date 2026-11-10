//! Security › Connectors and credentials in the admin console (admin console
//! spec): the organisation's apps with their status, how people reach them,
//! their scopes and when anyone last used them; add one by its address, turn
//! one off or on, set who may use it, remove one that was added; set, rotate
//! or revoke the organisation's credential for one. Credentials are
//! write-only for everyone, root included: no answer and no audit entry ever
//! holds one, only whether it is set and when.

use super::session::Admin;
use super::users::{actor, failed, plain, recorded, refuse};
use super::Console;
use axum::extract::{Path, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde_json::{json, Value};
use std::sync::Arc;

pub async fn list(State(c): State<Arc<Console>>) -> Response {
    match c.kernel.call(Method::GET, "/v1/admin/connectors", None).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => failed(e),
    }
}

async fn one(c: &Console, name: &str) -> Result<Value, Response> {
    if !plain(name) {
        return Err(refuse(StatusCode::NOT_FOUND, "not_found", "No such app."));
    }
    let v = c.kernel.call(Method::GET, "/v1/admin/connectors", None).await.map_err(failed)?;
    v["connectors"].as_array().into_iter().flatten().find(|x| x["name"] == name).cloned().ok_or_else(|| refuse(StatusCode::NOT_FOUND, "not_found", "No such app."))
}

/// What an admin set, as the audit log keeps it: everything but the key.
fn without_key(b: &Value) -> Value {
    json!({
        "url": b["url"], "audience": b["audience"], "label": b["label"], "access_groups": b["access_groups"],
        "key": if b["token"].as_str().is_some_and(|t| !t.trim().is_empty()) { json!("set") } else { Value::Null },
    })
}

/// Add an app by its MCP address, with what reaches it, what it returns,
/// who may use it, and the organisation's key when it takes one.
pub async fn add(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Json(b): Json<Value>) -> Response {
    let url = b["url"].as_str().map(str::trim).unwrap_or_default().to_string();
    if url.is_empty() {
        return refuse(StatusCode::BAD_REQUEST, "invalid", "Give the app's address.");
    }
    // A key or client details go on to the kernel, which refuses them with why.
    let mut body = json!({
        "url": url, "audience": b["audience"], "label": b["label"], "token": b["token"], "client_id": b["client_id"],
        "access_groups": b["access_groups"], "actor": actor(&a),
    });
    if body["access_groups"].is_null() {
        body["access_groups"] = json!([]);
    }
    match recorded(&c, &a, "connector_added", &url, None, Some(without_key(&body)), c.kernel.call(Method::POST, "/v1/admin/connectors", Some(&body))).await {
        Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
        Err(r) => r,
    }
}

/// Turn an app off or on for the organisation, or set who may use it.
pub async fn update(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(name): Path<String>, Json(b): Json<Value>) -> Response {
    let now = match one(&c, &name).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let enabled = b["enabled"].as_bool();
    let list = |f: &str| match b.get(f) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(g)) if g.iter().all(Value::is_string) => Ok(Some(g.clone())),
        Some(_) => Err(refuse(StatusCode::BAD_REQUEST, "invalid", "Who may use it, and who it is blocked for, are lists of groups.")),
    };
    let (groups, blocked) = match (list("access_groups"), list("blocked_groups")) {
        (Ok(g), Ok(x)) => (g, x),
        (Err(r), _) | (_, Err(r)) => return r,
    };
    if enabled.is_none() && groups.is_none() && blocked.is_none() {
        return refuse(StatusCode::BAD_REQUEST, "invalid", "Nothing to change.");
    }
    let before = json!({ "enabled": now["enabled"], "access_groups": now["access_groups"], "blocked_groups": now["blocked_groups"] });
    let after = json!({
        "enabled": enabled.map(Value::Bool).unwrap_or(now["enabled"].clone()),
        "access_groups": groups.clone().map(Value::Array).unwrap_or(now["access_groups"].clone()),
        "blocked_groups": blocked.clone().map(Value::Array).unwrap_or(now["blocked_groups"].clone()),
    });
    let body = json!({ "actor": actor(&a), "enabled": enabled, "access_groups": groups, "blocked_groups": blocked });
    match recorded(&c, &a, "connector_settings_changed", &name, Some(before), Some(after), c.kernel.call(Method::PATCH, &format!("/v1/admin/connectors/{name}"), Some(&body))).await
    {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// Remove an app that was added: every sign-in to it is revoked.
pub async fn remove(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(name): Path<String>) -> Response {
    let now = match one(&c, &name).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if now["source"] != "added" {
        let why = if now["source"] == "config" { "This app is set in the server's config file; change it there." } else { "This app is in the catalog: turn it off instead." };
        return refuse(StatusCode::CONFLICT, "not_removable", why);
    }
    let before = json!({ "url": now["url"], "signed_in": now["signed_in"] });
    match recorded(
        &c,
        &a,
        "connector_removed",
        &name,
        Some(before),
        None,
        c.kernel.call(Method::DELETE, &format!("/v1/admin/connectors/{name}"), Some(&json!({ "actor": actor(&a) }))),
    )
    .await
    {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// Set, or rotate, the organisation's credential for an app. The audit log
/// records that it was set, never what it is.
pub async fn set_credential(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(name): Path<String>, Json(b): Json<Value>) -> Response {
    let now = match one(&c, &name).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let Some(value) = b["value"].as_str().map(str::trim).filter(|v| !v.is_empty()) else { return refuse(StatusCode::BAD_REQUEST, "invalid", "Give the credential.") };
    let kind = now["credential"]["kind"].clone();
    let before = json!({ "kind": kind, "set": now["credential"]["set"] });
    let after = json!({ "kind": kind, "set": true });
    let body = json!({ "actor": actor(&a), "value": value });
    let action = if now["credential"]["set"] == true { "connector_credential_rotated" } else { "connector_credential_set" };
    match recorded(&c, &a, action, &name, Some(before), Some(after), c.kernel.call(Method::PUT, &format!("/v1/admin/connectors/{name}/credential"), Some(&body))).await {
        Ok(_) => Json(json!({ "set": true })).into_response(),
        Err(r) => r,
    }
}

/// Revoke the organisation's credential for an app, and every person's
/// sign-in to it.
pub async fn revoke_credential(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(name): Path<String>) -> Response {
    let now = match one(&c, &name).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let before = json!({ "kind": now["credential"]["kind"], "set": now["credential"]["set"], "signed_in": now["signed_in"] });
    let after = json!({ "kind": now["credential"]["kind"], "set": false, "signed_in": 0 });
    match recorded(
        &c,
        &a,
        "connector_credential_revoked",
        &name,
        Some(before),
        Some(after),
        c.kernel.call(Method::DELETE, &format!("/v1/admin/connectors/{name}/credential"), Some(&json!({ "actor": actor(&a) }))),
    )
    .await
    {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}
