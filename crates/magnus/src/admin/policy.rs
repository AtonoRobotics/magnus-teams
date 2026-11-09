//! Security › Agent policy in the admin console (admin console spec): each
//! Cedar policy set in an editor, checked against Magnus's schema with each
//! problem's line and column; a test panel (principal, action, resource,
//! context: the decision and the rules that decided it); a diff against the
//! live version before Apply; Apply and rollback with a fresh password, and
//! every version kept. A set's id here is its layer with `:` for `/`
//! (`org`, `group:sales`, `user:bob`), so it fits one path segment.

use super::session::Admin;
use super::users::{actor, failed, recorded, refuse};
use super::Console;
use axum::extract::{Path, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde_json::{json, Value};
use std::sync::Arc;

fn layer(id: &str) -> Option<String> {
    let l = id.replacen(':', "/", 1);
    let ok = l == "org"
        || l.strip_prefix("group/")
            .or(l.strip_prefix("user/"))
            .or(l.strip_prefix("task/"))
            .is_some_and(|n| !n.is_empty() && n.len() <= 100 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'));
    ok.then_some(l)
}

fn id_of(layer: &str) -> String {
    layer.replacen('/', ":", 1)
}

fn bad_id() -> Response {
    refuse(StatusCode::NOT_FOUND, "not_found", "A policy set is org, group:<name>, user:<person> or task:<task>.")
}

async fn sets_now(c: &Console) -> anyhow::Result<Vec<Value>> {
    Ok(listing(c).await?.0)
}

async fn listing(c: &Console) -> anyhow::Result<(Vec<Value>, Value)> {
    let v = c.kernel.call(Method::GET, "/v1/admin/policy", None).await?;
    let sets = v["sets"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|mut s| {
            s["layer"] = s["id"].clone();
            s["id"] = json!(id_of(s["id"].as_str().unwrap_or_default()));
            s
        })
        .collect();
    Ok((sets, json!({ "people": v["people"], "tools": v["tools"] })))
}

pub async fn sets(State(c): State<Arc<Console>>) -> Response {
    match listing(&c).await {
        Ok((s, names)) => Json(json!({ "sets": s, "schema": crate::policy_admin::SCHEMA.trim(), "people": names["people"], "tools": names["tools"] })).into_response(),
        Err(e) => failed(e),
    }
}

/// Parse and check against the schema; nothing is kept.
pub async fn validate(Json(b): Json<Value>) -> Response {
    let text = b["cedar"].as_str().unwrap_or_default();
    let problems = crate::policy_admin::check(text);
    Json(json!({ "ok": problems.is_empty(), "problems": problems })).into_response()
}

/// The set's draft as it is now, for the audit's before.
async fn draft_now(c: &Console, l: &str) -> Value {
    sets_now(c).await.ok().and_then(|s| s.into_iter().find(|x| x["layer"] == l)).map(|x| x["draft"].clone()).unwrap_or(Value::Null)
}

/// Keep a draft. It is not in force; the audit log records whose it is and
/// what it replaced, as for every console action.
pub async fn draft(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let Some(l) = layer(&id) else { return bad_id() };
    let Some(cedar) = b["cedar"].as_str() else { return refuse(StatusCode::BAD_REQUEST, "invalid", "Give the draft's text.") };
    let before = draft_now(&c, &l).await;
    match recorded(
        &c,
        &a,
        "policy_drafted",
        &id_of(&l),
        Some(json!({ "cedar": before })),
        Some(json!({ "cedar": cedar })),
        c.kernel.call(Method::POST, "/v1/admin/policy/draft", Some(&json!({ "actor": actor(&a), "layer": l, "cedar": cedar }))),
    )
    .await
    {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

pub async fn drop_draft(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>) -> Response {
    let Some(l) = layer(&id) else { return bad_id() };
    let before = draft_now(&c, &l).await;
    match recorded(
        &c,
        &a,
        "policy_draft_dropped",
        &id_of(&l),
        Some(json!({ "cedar": before })),
        None,
        c.kernel.call(Method::POST, "/v1/admin/policy/draft", Some(&json!({ "actor": actor(&a), "layer": l, "cedar": null }))),
    )
    .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(r) => r,
    }
}

/// Try a request against the live policy, or with the drafts in place.
pub async fn test(State(c): State<Arc<Console>>, Json(b): Json<Value>) -> Response {
    match c.kernel.call(Method::POST, "/v1/admin/policy/test", Some(&b)).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => failed(e),
    }
}

async fn put_in_force(c: &Console, a: &Admin, action: &str, l: &str, cedar: &str, note: Value) -> Response {
    let live = sets_now(c).await.ok().and_then(|s| s.into_iter().find(|x| x["layer"] == l)).map(|x| x["live"].clone()).unwrap_or(Value::Null);
    let after = json!({ "cedar": cedar, "how": note });
    match recorded(
        c,
        a,
        action,
        &id_of(l),
        Some(json!({ "cedar": live })),
        Some(after),
        c.kernel.call(Method::POST, "/v1/admin/policy/apply", Some(&json!({ "actor": actor(a), "layer": l, "cedar": cedar }))),
    )
    .await
    {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// Put the draft (or the text given) in force: a fresh password first, and
/// only a policy that fits the schema.
pub async fn apply(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>, body: Option<Json<Value>>) -> Response {
    let Some(l) = layer(&id) else { return bad_id() };
    if let Err(r) = a.step_up() {
        return r;
    }
    let given = body.and_then(|Json(b)| b["cedar"].as_str().map(String::from));
    let cedar = match given {
        Some(t) => t,
        None => match sets_now(&c).await {
            Ok(s) => match s.into_iter().find(|x| x["layer"] == l.as_str()).and_then(|x| x["draft"].as_str().map(String::from)) {
                Some(d) => d,
                None => return refuse(StatusCode::CONFLICT, "no_draft", "There is no draft to apply."),
            },
            Err(e) => return failed(e),
        },
    };
    let problems = crate::policy_admin::check(&cedar);
    if !problems.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid", "message": "The policy has problems: fix them before applying it.", "problems": problems })))
            .into_response();
    }
    put_in_force(&c, &a, "policy_applied", &l, &cedar, json!("applied")).await
}

pub async fn history(State(c): State<Arc<Console>>, Path(id): Path<String>) -> Response {
    let Some(l) = layer(&id) else { return bad_id() };
    let q = url::form_urlencoded::Serializer::new(String::new()).append_pair("layer", &l).finish();
    match c.kernel.call(Method::GET, &format!("/v1/admin/policy/history?{q}"), None).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => failed(e),
    }
}

/// Put an earlier version back in force, as a new version: a fresh
/// password first.
pub async fn rollback(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let Some(l) = layer(&id) else { return bad_id() };
    if let Err(r) = a.step_up() {
        return r;
    }
    let Some(version) = b["version"].as_u64() else { return refuse(StatusCode::BAD_REQUEST, "invalid", "Name the version to go back to.") };
    let q = url::form_urlencoded::Serializer::new(String::new()).append_pair("layer", &l).finish();
    let past = match c.kernel.call(Method::GET, &format!("/v1/admin/policy/history?{q}"), None).await {
        Ok(v) => v["versions"].as_array().into_iter().flatten().find(|x| x["version"].as_u64() == Some(version)).and_then(|x| x["cedar"].as_str().map(String::from)),
        Err(e) => return failed(e),
    };
    let Some(cedar) = past else { return refuse(StatusCode::NOT_FOUND, "not_found", "No such version.") };
    put_in_force(&c, &a, "policy_rolled_back", &l, &cedar, json!({ "to_version": version })).await
}

#[cfg(test)]
mod tests {
    /// A task's layer is a set the console edits like any other (review of #157).
    #[test]
    fn a_task_layer_is_a_policy_set() {
        assert_eq!(super::layer("task:task_0f3a").as_deref(), Some("task/task_0f3a"));
        assert_eq!(super::layer("group:ops").as_deref(), Some("group/ops"));
        assert!(super::layer("task:").is_none() && super::layer("task:a/b").is_none() && super::layer("tasks:x").is_none());
    }
}
