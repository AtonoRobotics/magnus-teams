//! The control routes that change one person's account: their role, status
//! and recovery address, turning them off and on, deleting them, and their
//! invitation. Each calls the kernel's own operation (`account_change`),
//! which asks the account rule itself; a route only reads the request.

use super::{actor, person_refused};
use crate::accounts::{Role, Status};
use crate::extract::Body;
use crate::kernel::Kernel;
use crate::people::Refusal;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::sync::Arc;

/// Who acts: the body's `actor`, which the Admin section always sets to the
/// signed-in admin. A request on the socket that names no one is the
/// console's, the server's own.
fn who(b: &Value) -> String {
    actor(b).unwrap_or_else(|| "console".into())
}

pub(super) fn refused((code, error, message): Refusal) -> Response {
    (code, Json(json!({ "error": error, "message": message }))).into_response()
}

fn invalid(message: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid", "message": message }))).into_response()
}

fn id_of(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// Change a role, a recovery address, a status, or several. Each is the
/// kernel's own operation, so each passes the account rule.
pub(super) async fn patch(State(k): State<Arc<Kernel>>, Path(id): Path<String>, Body(b): Body<Value>) -> Response {
    let id = id_of(&id);
    let actor = who(&b);
    if k.person(&id).is_none() {
        return refused(crate::account_change::not_found());
    }
    let role = match b["role"].as_str() {
        None => None,
        Some("admin") => Some(Role::Admin),
        Some("member") => Some(Role::Member),
        Some(_) => return invalid("The role is admin or member."),
    };
    let status = match b["status"].as_str() {
        None => None,
        Some("active") => Some(Status::Active),
        Some("disabled") => Some(Status::Disabled),
        Some(_) => return invalid("The status is active or disabled."),
    };
    if let Some(role) = role {
        if let Err(r) = k.set_directory_role(&actor, &id, role).await {
            return refused(r);
        }
    }
    // A recovery address: a string sets it, null or "" clears it.
    if let Some(email) = b.as_object().and_then(|o| o.get("email")) {
        let email = email.as_str().map(str::trim).filter(|e| !e.is_empty());
        if let Err(r) = k.change_account_email(&actor, &id, email).await {
            return refused(r);
        }
    }
    let done = match status {
        Some(Status::Disabled) => k.turn_off_account(&actor, &id).await.map(|_| ()),
        Some(_) => k.turn_on_account(&actor, &id).await,
        None => Ok(()),
    };
    match done {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(r) => refused(r),
    }
}

/// Turn someone off: no sign-in, their sessions ended on the record, their
/// schedules paused. Never the last owner or admin.
pub(super) async fn disable(State(k): State<Arc<Kernel>>, Path(id): Path<String>, Body(b): Body<Value>) -> Response {
    let Some(actor) = actor(&b) else { return (StatusCode::BAD_REQUEST, Json(json!({ "error": "name who turns them off" }))).into_response() };
    match k.turn_off_account(&actor, &id_of(&id)).await {
        Ok(off) => Json(json!({ "sessions_ended": off.sessions, "schedules_paused": off.schedules })).into_response(),
        Err(r) => refused(r),
    }
}

/// Turn them back on. Their paused schedules stay paused until they resume
/// them.
pub(super) async fn enable(State(k): State<Arc<Kernel>>, Path(id): Path<String>, Body(b): Body<Value>) -> Response {
    match k.turn_on_account(&who(&b), &id_of(&id)).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(r) => refused(r),
    }
}

/// Delete them: their content erased, then their local account where the
/// server keeps one. A config person cannot be unmade here, so they are
/// turned off instead and stay listed, disabled.
pub(super) async fn delete(State(k): State<Arc<Kernel>>, Path(id): Path<String>, Body(b): Body<Value>) -> Response {
    let Some(actor) = actor(&b) else { return (StatusCode::BAD_REQUEST, Json(json!({ "error": "name who deletes them" }))).into_response() };
    let id = id_of(&id);
    if k.person(&id).is_none() {
        return refused(crate::account_change::not_found());
    }
    if let Err(r) = k.may_change_account(&actor, &id).await {
        return refused(r);
    }
    // Disable first: it holds the last-admin guard, so a refused
    // deletion erases nothing — and a failed erasure leaves a listed,
    // disabled account whose deletion retries cleanly, instead of a
    // vanished person with orphaned content (#529).
    let local = k.accounts.is_local(&id);
    let ended = match k.accounts.set_status(&id, Status::Disabled).await {
        Ok(ended) => ended,
        Err(e) => return refused(crate::account_change::store_refusal(e)),
    };
    // Sessions the disabling ended are signed out on the record, as a
    // disable signs its own.
    if !ended.is_empty() {
        let evs: Vec<(&str, crate::events::Ev)> =
            ended.iter().map(|session| (actor.as_str(), crate::events::Ev::SignedOut { person: id.clone(), session: session.clone() })).collect();
        if let Err(e) = k.emit_all(evs).await {
            return refused(crate::account_change::store_refusal(e));
        }
    }
    let mut erased = match k.erase_person(&actor, &id).await {
        Ok(v) => v,
        Err(r) => return person_refused(r),
    };
    // The disabling already ended their sessions; the erasure counts
    // its own. The answer counts every session the deletion ended.
    erased["sessions_ended"] = json!(ended.len() as u64 + erased["sessions_ended"].as_u64().unwrap_or(0));
    // Then the local directory entry goes, its content already erased. A
    // disabled account is never the last enabled admin, so this only
    // fails the store itself — leaving a listed, disabled shell whose
    // deletion retries (the erasure finds nothing new) and removes.
    if local {
        if let Err(e) = k.remove_local_person(&id).await {
            return refused(crate::account_change::store_refusal(e));
        }
    }
    Json(json!({ "erased": erased, "account_removed": local })).into_response()
}

/// Withdraw an invitation sent from here: the local account goes, never
/// having held anything. Only a local invited person can be withdrawn.
pub(super) async fn cancel_invite(State(k): State<Arc<Kernel>>, Path(id): Path<String>, Body(b): Body<Value>) -> Response {
    match k.withdraw_invitation(&who(&b), &id_of(&id)).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(r) => refused(r),
    }
}

/// Send the invitation again: a fresh token, shown once here; the old
/// one dies with it. Only an invitation still waiting.
pub(super) async fn resend_invite(State(k): State<Arc<Kernel>>, Path(id): Path<String>, Body(b): Body<Value>) -> Response {
    let id = id_of(&id);
    match k.renew_invitation(&who(&b), &id).await {
        Ok(token) => Json(json!({ "id": id, "invite_token": token, "invite_expires": chrono::Utc::now() + chrono::Duration::days(crate::accounts::INVITE_DAYS) })).into_response(),
        Err(r) => refused(r),
    }
}

/// Give an invited person a password to choose with once, shown to the
/// admin here only. It never signs in; choosing with it sets their own.
pub(super) async fn invite_password(State(k): State<Arc<Kernel>>, Path(id): Path<String>, Body(b): Body<Value>) -> Response {
    let password = b["password"].as_str().unwrap_or_default();
    match k.set_invitation_password(&who(&b), &id_of(&id), password).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(r) => refused(r),
    }
}
