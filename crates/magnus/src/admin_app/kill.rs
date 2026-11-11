//! The kill switch in the admin's top bar (spec rev 141, Kill switch), at
//! the server level: the one switch the app's Settings and the console move
//! (`Kernel::kill_switch`), reached through the console's control route.
//! Pressing needs no step-up; releasing does (the guard's rule).
//!
//! Levels to come (one person's agents, one agent, one connector) mount
//! beside it as `/v1/admin/kill-switch/<level>/<id>/press` and `.../release`,
//! which the guard already treats the same way, and add what they hold
//! stopped to [`stops`], so the banner reaches the people they affect.

use super::mount::{audited, body_json, control, record, Audit};
use super::Admin;
use crate::kernel::Kernel;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) fn routes() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/admin/kill-switch", get(state)).route("/v1/admin/kill-switch/press", post(press)).route("/v1/admin/kill-switch/release", post(release))
}

/// The server's switch as the console shows it, with the name of who moved it.
async fn state(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>) -> Response {
    let mut v = body_json(control(&k, &a, Method::GET, "/v1/kill-switch", None).await).await;
    v["by_name"] = json!(v["by"].as_str().map(|by| name_of(&k, by)));
    Json(v).into_response()
}

/// Stop everything, at once and without a step-up. The switch records itself
/// even when nothing else can be; the audit entry follows, as the console's.
async fn press(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>) -> Response {
    let r = control(&k, &a, Method::POST, "/v1/kill-switch", Some(json!({ "engaged": true }))).await;
    if !r.status().is_success() {
        return r;
    }
    let v = body_json(r).await;
    if let Err(why) = record(&k, &a, &Audit { action: "kill_switch_engaged".into(), target: Some("server".into()), before: None, after: Some(v.clone()) }).await {
        let msg = format!("The kill switch is on, but its audit entry could not be written ({why}).");
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "audit", "message": msg, "switch": v }))).into_response();
    }
    Json(v).into_response()
}

/// Release it: after a step-up (the guard's), recorded first.
async fn release(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>) -> Response {
    let audit = Audit { action: "kill_switch_released".into(), target: Some("server".into()), before: Some(json!({ "engaged": true })), after: Some(json!({ "engaged": false })) };
    audited(&k, &a, audit, control(&k, &a, Method::POST, "/v1/kill-switch", Some(json!({ "engaged": false })))).await
}

/// A person's name from a log actor (`person:<id>`, `admin:<id>`), or the
/// actor itself (`kernel`).
fn name_of(k: &Kernel, actor: &str) -> String {
    let id = actor.split_once(':').map(|(_, id)| id).unwrap_or(actor);
    match k.person(id) {
        Some(p) if !p.name.is_empty() => p.name,
        Some(p) => p.id,
        None => actor.to_string(),
    }
}

/// The stops `GET /v1/stops` lists: each switch that holds the person's work
/// stopped, with who pulled it and when, for the banner the app shows to the
/// admins and to the people affected. Each is
/// `{level: "server"|"person"|"agent"|"connector", target, by, by_name, at}`.
fn stops_for(k: &Kernel, _person: &str) -> Vec<Value> {
    let server = k.read(|s| s.kill_switch.then(|| s.kill_switch_by.clone()));
    let mut out = vec![];
    if let Some(by) = server {
        let (actor, at) = by.map(|(who, at)| (Some(who), Some(at))).unwrap_or((None, None));
        out.push(json!({ "level": "server", "target": Value::Null, "by": actor, "by_name": actor.as_deref().map(|a| name_of(k, a)), "at": at }));
    }
    out
}

/// `GET /v1/stops`: the kill switches on that affect the signed-in person.
/// Any signed-in person may ask; it names only who pulled what affects them.
pub(super) async fn stops(State(k): State<Arc<Kernel>>, h: HeaderMap) -> Response {
    match crate::api::who(&k, &h).await {
        Ok(p) => Json(json!({ "stops": stops_for(&k, &p.id) })).into_response(),
        Err(r) => r,
    }
}
