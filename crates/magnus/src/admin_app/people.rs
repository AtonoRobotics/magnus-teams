//! People in the Admin section (spec rev 141, People): invite by email or
//! link, set each person's role (owner, admin, member or guest), turn an
//! account off, sign them out of every device, and remove them. Removing
//! transfers or deletes their projects and agents and deletes their data
//! (`remove`), and the last owner cannot be removed, turned off or demoted.
//! Every admin read of a person's data is logged and the person is told:
//! the control route does both (`Kernel::read_person_data`), and the read is
//! in the admin audit too.
//!
//! Each change goes through the console's control route where one exists, so
//! its checks (the last admin, the sessions ended on the record) are the
//! same; what the console lacked is here.

use super::mount::{audited, body_json, control, Audit};
use super::roles::role_of;
use super::Admin;
use crate::kernel::Kernel;
use axum::extract::{Path, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) fn routes() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/admin/people", get(list).post(invite))
        .route("/v1/admin/people/{id}", get(one).patch(change).delete(super::remove::remove))
        .route("/v1/admin/people/{id}/disable", post(disable))
        .route("/v1/admin/people/{id}/enable", post(super::mount::pass(Some("user_enabled"))))
        .route("/v1/admin/people/{id}/sign-out", post(sign_out))
        .route("/v1/admin/people/{id}/sessions", get(sessions))
        .route("/v1/admin/people/{id}/sessions/{sid}", delete(end_session))
        .route("/v1/admin/people/{id}/invite", post(resend))
        .route("/v1/admin/people/{id}/cancel-invite", post(super::mount::pass(Some("invite_cancelled"))))
        .route("/v1/admin/people/{id}/invite/password", post(super::mount::pass(Some("invite_password_set"))))
        .route("/v1/admin/people/{id}/schedules", get(schedules))
        .route("/v1/admin/people/{id}/schedules/{sid}/{change}", post(schedule))
        .route("/v1/admin/people/{id}/data/{what}", post(read_data))
}

pub(super) fn refuse(code: StatusCode, error: &str, message: &str) -> Response {
    (code, Json(json!({ "error": error, "message": message }))).into_response()
}

fn id_of(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// A person row as the control route gives it, with their role of the four.
async fn with_role(k: &Kernel, mut row: Value) -> Value {
    let id = row["id"].as_str().unwrap_or_default().to_string();
    if let Some(p) = k.person(&id) {
        row["role"] = json!(role_of(&p, k.accounts.role(&id).await));
        row["roles"] = json!(p.roles);
    }
    row
}

async fn list(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>) -> Response {
    let r = control(&k, &a, Method::GET, "/v1/admin/people", None).await;
    if !r.status().is_success() {
        return r;
    }
    let v = body_json(r).await;
    let mut rows = vec![];
    for row in v["people"].as_array().cloned().unwrap_or_default() {
        rows.push(with_role(&k, row).await);
    }
    Json(json!({ "people": rows, "roles": super::roles::ROLES })).into_response()
}

async fn one(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(id): Path<String>) -> Response {
    let r = control(&k, &a, Method::GET, &format!("/v1/admin/people/{}", id_of(&id)), None).await;
    if !r.status().is_success() {
        return r;
    }
    Json(with_role(&k, body_json(r).await).await).into_response()
}

/// Only an owner gives or takes the owner role.
fn owner_only(a: &Admin, role: &str, target_is_owner: bool) -> Option<Response> {
    ((role == "owner" || target_is_owner) && !a.person.is_owner()).then(|| refuse(StatusCode::FORBIDDEN, "owner_only", "Only an owner gives or takes the owner role."))
}

/// Set a role on the record, audited as the console's `role_changed`.
async fn set_role(k: &Arc<Kernel>, a: &Admin, id: &str, role: &str) -> Response {
    let before = k.person(id).map(|p| role_of(&p, crate::accounts::Role::Member)).map(|r| json!(r));
    let audit = Audit { action: "role_changed".into(), target: Some(format!("people/{id}")), before, after: Some(json!({ "role": role })) };
    let change = async {
        match k.set_server_role(&a.actor(), id, role).await {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err((code, error, message)) => refuse(code, error, &message),
        }
    };
    audited(k, a, audit, change).await
}

/// Invite someone by email: their account and a single-use link, which the
/// admin passes on, and which goes to them by email when the server sends
/// mail and the admin asked for it. With no email, a single-use link for
/// whoever opens it to make their own account (`account_links`).
async fn invite(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Json(mut b): Json<Value>) -> Response {
    let role = b["role"].as_str().unwrap_or("member").to_string();
    if !super::roles::ROLES.contains(&role.as_str()) {
        return refuse(StatusCode::BAD_REQUEST, "invalid", "The role is owner, admin, member or guest.");
    }
    if let Some(r) = owner_only(&a, &role, false) {
        return r;
    }
    if b["email"].as_str().map(str::trim).unwrap_or_default().is_empty() {
        let audit = Audit { action: "user_invited".into(), target: None, before: None, after: Some(json!({ "link": true, "role": role })) };
        return audited(&k, &a, audit, crate::account_links::made(&k, &a.actor(), &role)).await;
    }
    b["role"] = json!(if matches!(role.as_str(), "owner" | "admin") { "admin" } else { "member" });
    let send = b["send"].as_bool().unwrap_or(false);
    let audit = Audit {
        action: "user_invited".into(),
        target: b["email"].as_str().map(|e| format!("people/{e}")),
        before: None,
        after: Some(json!({ "email": b["email"], "name": b["name"], "role": role })),
    };
    let r = audited(&k, &a, audit, control(&k, &a, Method::POST, "/v1/admin/people", Some(b))).await;
    if !r.status().is_success() {
        return r;
    }
    let mut v = body_json(r).await;
    let id = v["id"].as_str().unwrap_or_default().to_string();
    if let Err((code, error, message)) = k.set_server_role(&a.actor(), &id, &role).await {
        return refuse(code, error, &message);
    }
    v["role"] = json!(role);
    deliver(&k, &a, &mut v, send).await;
    (StatusCode::CREATED, Json(v)).into_response()
}

/// The invitation's link, and whether it went out by email.
async fn deliver(k: &Kernel, a: &Admin, v: &mut Value, send: bool) {
    let link = v["invite_token"].as_str().and_then(|t| crate::invitations::invite_link(&k.cfg, t));
    v["link"] = json!(link);
    v["emailed"] = json!(false);
    let (Some(link), Some(to), true) = (link, v["email"].as_str().map(String::from), send) else { return };
    let sent = crate::invitations::email(k, &a.name(), &to, "Magnus", &link).await;
    v["emailed"] = json!(sent.emailed);
    if let Some(e) = sent.error {
        v["email_error"] = json!(e);
    }
}

/// Send the invitation again: a fresh link, the old one dead.
async fn resend(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(id): Path<String>, body: Option<Json<Value>>) -> Response {
    let id = id_of(&id);
    let send = body.as_ref().and_then(|b| b["send"].as_bool()).unwrap_or(false);
    let audit = Audit { action: "invite_resent".into(), target: Some(format!("people/{id}")), ..Default::default() };
    let r = audited(&k, &a, audit, control(&k, &a, Method::POST, &format!("/v1/admin/people/{id}/invite"), None)).await;
    if !r.status().is_success() {
        return r;
    }
    let mut v = body_json(r).await;
    v["email"] = json!(k.accounts.local_person(&id).and_then(|l| l.email));
    deliver(&k, &a, &mut v, send).await;
    Json(v).into_response()
}

/// A role, a recovery address or a status, each the way its own route does it.
async fn change(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(id): Path<String>, Json(mut b): Json<Value>) -> Response {
    let id = id_of(&id);
    let Some(target) = k.person(&id) else { return refuse(StatusCode::NOT_FOUND, "not_found", "No such person.") };
    if let Some(role) = b.as_object_mut().and_then(|o| o.remove("role")) {
        let Some(role) = role.as_str() else { return refuse(StatusCode::BAD_REQUEST, "invalid", "The role is owner, admin, member or guest.") };
        if let Some(r) = owner_only(&a, role, target.is_owner()) {
            return r;
        }
        let r = set_role(&k, &a, &id, role).await;
        if !r.status().is_success() {
            return r;
        }
    }
    if b["status"] == "disabled" && k.last_owner(&id).await {
        return last_owner();
    }
    if b.as_object().is_some_and(|o| o.contains_key("email") || o.contains_key("status")) {
        let audit = Audit { action: "user_updated".into(), target: Some(format!("people/{id}")), before: None, after: Some(b.clone()) };
        return audited(&k, &a, audit, control(&k, &a, Method::PATCH, &format!("/v1/admin/people/{id}"), Some(b))).await;
    }
    StatusCode::NO_CONTENT.into_response()
}

pub(super) fn last_owner() -> Response {
    refuse(StatusCode::CONFLICT, "last_owner", "This is the last owner: make someone else an owner first.")
}

/// Turn someone off: never the last owner, and an owner only by an owner.
async fn disable(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(id): Path<String>) -> Response {
    let id = id_of(&id);
    if k.last_owner(&id).await {
        return last_owner();
    }
    if let Some(r) = owner_only(&a, "", k.person(&id).is_some_and(|p| p.is_owner())) {
        return r;
    }
    let audit = Audit { action: "user_disabled".into(), target: Some(format!("people/{id}")), ..Default::default() };
    audited(&k, &a, audit, control(&k, &a, Method::POST, &format!("/v1/admin/people/{id}/disable"), None)).await
}

/// Sign them out of every device: each session ends on the record.
async fn sign_out(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(id): Path<String>) -> Response {
    let id = id_of(&id);
    if k.person(&id).is_none() {
        return refuse(StatusCode::NOT_FOUND, "not_found", "No such person.");
    }
    let audit = Audit { action: "sessions_ended".into(), target: Some(format!("people/{id}")), ..Default::default() };
    audited(&k, &a, audit, control(&k, &a, Method::DELETE, &format!("/v1/people/{id}/sessions"), None)).await
}

async fn sessions(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(id): Path<String>) -> Response {
    control(&k, &a, Method::GET, &format!("/v1/people/{}/sessions", id_of(&id)), None).await
}

async fn end_session(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path((id, sid)): Path<(String, String)>) -> Response {
    let id = id_of(&id);
    let audit = Audit { action: "session_ended".into(), target: Some(format!("people/{id}/sessions/{sid}")), ..Default::default() };
    audited(&k, &a, audit, control(&k, &a, Method::DELETE, &format!("/v1/people/{id}/sessions/{sid}"), None)).await
}

async fn schedules(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(id): Path<String>) -> Response {
    control(&k, &a, Method::GET, &format!("/v1/people/{}/schedules", id_of(&id)), None).await
}

async fn schedule(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path((id, sid, change)): Path<(String, String, String)>) -> Response {
    let id = id_of(&id);
    let action = if change == "resume" { "schedule_resumed" } else { "schedule_suspended" };
    let audit = Audit { action: action.into(), target: Some(format!("people/{id}/schedules/{sid}")), ..Default::default() };
    audited(&k, &a, audit, control(&k, &a, Method::POST, &format!("/v1/people/{id}/schedules/{sid}/{change}"), Some(json!({ "name": a.name() })))).await
}

/// Read a person's data: in the audit as `person_data_read` first, then in
/// the log and the person's inbox by the read itself.
async fn read_data(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path((id, what)): Path<(String, String)>, body: Option<Json<Value>>) -> Response {
    let id = id_of(&id);
    let item = body.as_ref().and_then(|b| b["item"].as_str().map(String::from));
    let audit = Audit { action: "person_data_read".into(), target: Some(format!("people/{id}/data/{what}")), before: None, after: item.as_ref().map(|i| json!({ "item": i })) };
    let body = json!({ "name": a.name(), "item": item });
    audited(&k, &a, audit, control(&k, &a, Method::POST, &format!("/v1/people/{id}/data/{what}"), Some(body))).await
}
