//! Users in the admin console (admin console spec, Users): everyone the
//! appliance keeps, invited from here, their role, whether they are
//! turned on, and the sessions they hold here. Changing a role needs a
//! fresh password, and so does inviting an admin; the last admin cannot
//! be demoted, turned off or removed — the kernel refuses, so the guard
//! holds across the console and the owner's shell alike. Every change is
//! recorded before it is made.

use super::session::Admin;
use super::Console;
use axum::extract::{Path, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) fn failed(e: anyhow::Error) -> Response {
    if let Some(r) = e.downcast_ref::<super::kernel::Refused>() {
        let status = StatusCode::from_u16(r.status).unwrap_or(StatusCode::BAD_REQUEST);
        // The kernel's own refusals pass through under their names, with
        // what it said; anything else is a refusal as such.
        let error = match r.message.as_str() {
            x @ ("last_admin" | "not_invited" | "taken" | "not_found" | "invalid") => x,
            _ => "refused",
        };
        let message = match error {
            "refused" => r.message.clone(),
            _ if r.detail.is_empty() => r.message.clone(),
            _ => r.detail.clone(),
        };
        return (status, Json(json!({ "error": error, "message": message }))).into_response();
    }
    (StatusCode::BAD_GATEWAY, Json(json!({ "error": "unavailable", "message": format!("{e:#}") }))).into_response()
}

pub(super) fn refuse(status: StatusCode, error: &str, message: &str) -> Response {
    (status, Json(json!({ "error": error, "message": message }))).into_response()
}

/// Record the change, then make it: nothing changes that the audit log
/// does not hold. A change that then fails is recorded as failed too.
pub(super) async fn recorded<F>(c: &Console, a: &Admin, action: &str, target: &str, before: Option<Value>, after: Option<Value>, change: F) -> Result<Value, Response>
where
    F: std::future::Future<Output = anyhow::Result<Value>>,
{
    if let Err(e) = c.audit((&a.session.sub, &a.session.name), action, Some(target), a.peer, before, after).await {
        return Err(refuse(StatusCode::SERVICE_UNAVAILABLE, "audit", &format!("{e:#}")));
    }
    match change.await {
        Ok(v) => Ok(v),
        Err(e) => {
            let _ = c.audit((&a.session.sub, &a.session.name), &format!("{action}_failed"), Some(target), a.peer, None, Some(json!({ "error": format!("{e:#}") }))).await;
            Err(failed(e))
        }
    }
}

/// An id as the kernel makes them: nothing that could become another
/// path. Dots are ids too (`dee.ray`), but never a path of their own.
pub(super) fn plain(id: &str) -> bool {
    !id.is_empty() && id != "." && id != ".." && id.len() <= 100 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

pub(super) fn actor(a: &Admin) -> String {
    format!("admin:{}", a.session.sub)
}

/// Everyone the appliance keeps, with their role, status, last sign-in
/// and open sessions.
pub async fn list(State(c): State<Arc<Console>>) -> Response {
    match c.kernel.call(Method::GET, "/v1/admin/people", None).await {
        Ok(v) => Json(json!({ "users": v["people"] })).into_response(),
        Err(e) => failed(e),
    }
}

async fn one(c: &Console, id: &str) -> Result<(Value, bool), Response> {
    if !plain(id) {
        return Err(refuse(StatusCode::NOT_FOUND, "not_found", "No such person."));
    }
    let p = c.kernel.call(Method::GET, &format!("/v1/admin/people/{id}"), None).await.map_err(failed)?;
    let admin = p["role"] == "admin";
    Ok((p, admin))
}

/// One person, with each session they hold here.
pub async fn detail(State(c): State<Arc<Console>>, Path(id): Path<String>) -> Response {
    let (mut p, _) = match one(&c, &id).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let s = c.kernel.call(Method::GET, &format!("/v1/people/{id}/sessions"), None).await;
    p["sessions"] = s.map(|v| v["sessions"].clone()).unwrap_or(Value::Null);
    Json(p).into_response()
}

fn role_of(b: &Value) -> Option<bool> {
    match b["role"].as_str() {
        Some("admin") => Some(true),
        Some("member") => Some(false),
        _ => None,
    }
}

/// Invite someone: the appliance makes their account and a single-use
/// token, shown once here for the admin to pass on — the appliance
/// cannot send mail, so no email is ever sent (#508). Inviting an admin
/// needs a fresh password.
pub async fn invite(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Json(b): Json<Value>) -> Response {
    let email = b["email"].as_str().map(str::trim).unwrap_or_default().to_lowercase();
    let name = b["name"].as_str().map(str::trim).unwrap_or_default().to_string();
    let Some(admin) = role_of(&b) else { return refuse(StatusCode::BAD_REQUEST, "invalid", "The role is admin or member.") };
    if !valid_email(&email) || name.is_empty() || name.len() > 200 {
        return refuse(StatusCode::BAD_REQUEST, "invalid", "An invitation needs an email address and a name.");
    }
    if admin {
        if let Err(r) = a.step_up() {
            return r;
        }
    }
    let after = json!({ "email": email, "name": name, "role": if admin { "admin" } else { "member" } });
    let body = json!({ "email": email, "name": name, "role": if admin { "admin" } else { "member" } });
    let r = recorded(&c, &a, "user_invited", &email, None, Some(after), c.kernel.call(Method::POST, "/v1/admin/people", Some(&body))).await;
    match r {
        Ok(v) => (
            StatusCode::CREATED,
            Json(json!({ "id": v["id"], "email": v["email"], "invite_token": v["invite_token"], "invite_expires": v["invite_expires"], "email_sent": false })),
        )
            .into_response(),
        Err(r) => r,
    }
}

fn valid_email(e: &str) -> bool {
    let Some((user, host)) = e.split_once('@') else { return false };
    !user.is_empty()
        && host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && e.len() <= 254
        && !e.chars().any(|c| c.is_whitespace() || c == '<' || c == '>' || c == ',')
}

/// Change a role: a fresh password, and never the last admin's — the
/// kernel refuses, so two such changes at once still leave one (#192).
pub async fn update(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let Some(to_admin) = role_of(&b) else { return refuse(StatusCode::BAD_REQUEST, "invalid", "The role is admin or member.") };
    if let Err(r) = a.step_up() {
        return r;
    }
    let (_, is_admin) = match one(&c, &id).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if is_admin == to_admin {
        return StatusCode::NO_CONTENT.into_response();
    }
    let role = |admin: bool| json!({ "role": if admin { "admin" } else { "member" } });
    let body = json!({ "role": if to_admin { "admin" } else { "member" } });
    match recorded(&c, &a, "user_role_changed", &id, Some(role(is_admin)), Some(role(to_admin)), c.kernel.call(Method::PATCH, &format!("/v1/admin/people/{id}"), Some(&body))).await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(r) => r,
    }
}

/// Turn someone off: here refuses them, their sessions end on the
/// record, and their schedules pause. Never the last admin.
pub async fn disable(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>) -> Response {
    if let Err(r) = one(&c, &id).await {
        return r;
    }
    let r = recorded(
        &c,
        &a,
        "user_disabled",
        &id,
        Some(json!({ "enabled": true })),
        Some(json!({ "enabled": false })),
        c.kernel.call(Method::POST, &format!("/v1/admin/people/{id}/disable"), Some(&json!({ "actor": actor(&a) }))),
    )
    .await;
    match r {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// Turn them back on. Their paused schedules stay paused until they resume
/// them; turning them on also unlocks them, out of a lockout.
pub async fn enable(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>) -> Response {
    if let Err(r) = one(&c, &id).await {
        return r;
    }
    let r = recorded(
        &c,
        &a,
        "user_enabled",
        &id,
        Some(json!({ "enabled": false })),
        Some(json!({ "enabled": true })),
        c.kernel.call(Method::POST, &format!("/v1/admin/people/{id}/enable"), None),
    )
    .await;
    match r {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(r) => r,
    }
}

/// End all their sessions: every device signed in here.
pub async fn end_sessions(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>) -> Response {
    if let Err(r) = one(&c, &id).await {
        return r;
    }
    let r =
        recorded(&c, &a, "user_sessions_ended", &id, None, None, c.kernel.call(Method::DELETE, &format!("/v1/people/{id}/sessions"), Some(&json!({ "actor": actor(&a) })))).await;
    match r {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// End one session here: that device is signed out.
pub async fn end_session(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path((id, sid)): Path<(String, String)>) -> Response {
    if !plain(&id) || !plain(&sid) {
        return refuse(StatusCode::NOT_FOUND, "not_found", "No such session.");
    }
    let target = format!("{id}/{sid}");
    let r = recorded(&c, &a, "user_session_ended", &target, None, None, async {
        c.kernel.call(Method::DELETE, &format!("/v1/people/{id}/sessions/{sid}"), Some(&json!({ "actor": actor(&a) }))).await
    })
    .await;
    match r {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

async fn invited(c: &Console, id: &str) -> Result<Value, Response> {
    let (p, _) = one(c, id).await?;
    if p["status"] != "invited" {
        return Err(refuse(StatusCode::CONFLICT, "not_invited", "They have already chosen their password: this is an account, not an invitation."));
    }
    Ok(p)
}

/// Send the invitation again: a fresh token, shown once here; the old
/// one dies with it.
pub async fn resend(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>) -> Response {
    if let Err(r) = invited(&c, &id).await {
        return r;
    }
    match recorded(&c, &a, "user_invite_resent", &id, None, None, c.kernel.call(Method::POST, &format!("/v1/admin/people/{id}/invite"), None)).await {
        Ok(v) => Json(json!({ "id": v["id"], "invite_token": v["invite_token"], "invite_expires": v["invite_expires"] })).into_response(),
        Err(r) => r,
    }
}

/// When no invite link can reach them: a password to choose with once,
/// shown to the admin here only. It never signs in; choosing with it
/// sets their own. Only for an invitation, and for an admin's with a
/// fresh password, as inviting one needs.
pub async fn one_time_password(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>) -> Response {
    let p = match invited(&c, &id).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if p["role"] == "admin" {
        if let Err(r) = a.step_up() {
            return r;
        }
    }
    let password = one_time();
    let r = recorded(
        &c,
        &a,
        "user_invite_password_set",
        &id,
        None,
        None,
        c.kernel.call(Method::POST, &format!("/v1/admin/people/{id}/invite/password"), Some(&json!({ "password": password }))),
    )
    .await;
    match r {
        Ok(_) => Json(json!({ "password": password, "username": p["username"] })).into_response(),
        Err(r) => r,
    }
}

/// Twenty letters and digits from the system's randomness.
fn one_time() -> String {
    use rand::Rng;
    rand::rngs::OsRng.sample_iter(&rand::distributions::Alphanumeric).take(20).map(char::from).collect()
}

/// Withdraw an invitation nobody has taken up: the account goes, never
/// having held anything. An account in use is removed only by Delete.
/// An invited admin is always surplus — the last admin turned on can
/// never be only invited — so the kernel's guard is the whole check.
pub async fn cancel(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>) -> Response {
    let p = match invited(&c, &id).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match recorded(
        &c,
        &a,
        "user_invite_cancelled",
        &id,
        Some(json!({ "email": p["email"], "role": p["role"] })),
        None,
        c.kernel.call(Method::POST, &format!("/v1/admin/people/{id}/cancel-invite"), None),
    )
    .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(r) => r,
    }
}

/// Their schedules (admin console spec, Users › Schedules and triggers).
pub async fn schedules(State(c): State<Arc<Console>>, Path(id): Path<String>) -> Response {
    if let Err(r) = one(&c, &id).await {
        return r;
    }
    match c.kernel.call(Method::GET, &format!("/v1/people/{id}/schedules"), None).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => failed(e),
    }
}

/// Suspend or resume one of their schedules.
pub async fn schedule(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path((id, sid, change)): Path<(String, String, String)>) -> Response {
    if !plain(&sid) || !matches!(change.as_str(), "suspend" | "resume") {
        return refuse(StatusCode::NOT_FOUND, "not_found", "No such schedule.");
    }
    if let Err(r) = one(&c, &id).await {
        return r;
    }
    let on = change == "resume";
    let action = if on { "user_schedule_resumed" } else { "user_schedule_suspended" };
    let body = json!({ "actor": actor(&a), "name": a.session.name });
    match recorded(
        &c,
        &a,
        action,
        &format!("{id}/{sid}"),
        Some(json!({ "suspended": on })),
        Some(json!({ "suspended": !on })),
        c.kernel.call(Method::POST, &format!("/v1/people/{id}/schedules/{sid}/{change}"), Some(&body)),
    )
    .await
    {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// Root's reading of their tasks, conversations, files or memory, or one of
/// them (admin console spec, Users › Data; owner's ruling 14). Each read is
/// in the audit log, with the admin, the person, what and when, before it
/// is made; the kernel records it too and tells the person.
pub async fn data(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(p): Path<Vec<(String, String)>>) -> Response {
    let get = |k: &str| p.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let (Some(id), Some(kind)) = (get("id"), get("kind")) else { return refuse(StatusCode::NOT_FOUND, "not_found", "Nothing here.") };
    let item = get("item");
    if !crate::person_admin::KINDS.contains(&kind.as_str()) || item.as_deref().is_some_and(|i| !plain(i)) {
        return refuse(StatusCode::NOT_FOUND, "not_found", "Their tasks, chats, files or memory.");
    }
    if let Err(r) = one(&c, &id).await {
        return r;
    }
    let body = json!({ "actor": actor(&a), "name": a.session.name, "item": item });
    let read = json!({ "resource": kind, "item": item });
    match recorded(&c, &a, "user_data_read", &id, None, Some(read), c.kernel.call(Method::POST, &format!("/v1/people/{id}/data/{kind}"), Some(&body))).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// Delete them (admin console spec, Users › Delete): their data first (the
/// kernel stops their runs, ends their schedules, sessions and sign-ins,
/// and removes their content from the store and the backups), then their
/// local account where the appliance keeps one. A fresh password, their
/// email typed, and never themselves nor the last admin.
pub async fn delete(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    if let Err(r) = a.step_up() {
        return r;
    }
    let (p, _) = match one(&c, &id).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let typed = b["email"].as_str().map(|e| e.trim().to_lowercase()).unwrap_or_default();
    // Typed back: their email address, or their id when they have no
    // email (a config person the appliance does not keep an address for).
    match p["email"].as_str() {
        Some(e) if e.to_lowercase() == typed => {}
        Some(_) => return refuse(StatusCode::BAD_REQUEST, "confirm", "Type their email address to delete them."),
        None if typed == id.to_lowercase() => {}
        None => return refuse(StatusCode::BAD_REQUEST, "confirm", "They have no email address: type their id to delete them."),
    }
    if id == a.session.sub {
        return refuse(StatusCode::CONFLICT, "self", "You cannot delete yourself: another admin can.");
    }
    let before = json!({ "name": p["name"], "email": p["email"], "role": p["role"] });
    let body = json!({ "actor": actor(&a) });
    let r = recorded(&c, &a, "user_deleted", &id, Some(before), None, c.kernel.call(Method::DELETE, &format!("/v1/admin/people/{id}"), Some(&body))).await;
    match r {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_id_never_becomes_another_path() {
        assert!(super::plain("0b6c1f2e-6c1d-4a1e-9b1a-3f2c1d0e9a8b") && super::plain("Zx_9-a") && super::plain("dee.ray"));
        for bad in ["", ".", "..", "a/b", "a%2Fb", "x/../../audit", "a?b", &"a".repeat(101)] {
            assert!(!super::plain(bad), "{bad}");
        }
    }

    #[test]
    fn an_invitation_goes_to_an_email_address() {
        for ok in ["al@acme.test", "a.b+c@mail.acme.test"] {
            assert!(super::valid_email(ok), "{ok}");
        }
        for bad in ["", "al", "@acme.test", "al@acme", "al@.acme.test", "al @acme.test", "al@acme.test, bo@acme.test", "<al@acme.test>"] {
            assert!(!super::valid_email(bad), "{bad}");
        }
    }
}
