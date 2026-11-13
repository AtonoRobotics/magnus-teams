//! Admin changes outside the Admin section (spec rev 141, Administration:
//! "Each admin change asks for the admin's password and code again if they
//! have not confirmed them in the last 5 minutes").
//!
//! The app's older owner routes (Settings' models, connectors, roles,
//! updates, backups, secrets and kill switch, and what a deployment calls)
//! still answer, so a client that predates the Admin section keeps working,
//! but none of them changes anything for an admin without the same step-up
//! and the same audit entry as the Admin section's own routes. [`gate`] is a
//! layer on the whole app router: a request matching [`ADMIN_CHANGES`] from
//! an owner or admin is refused with `403 {"error":"step_up"}` unless their
//! session stepped up within `ADMIN_STEP_UP_SECONDS`, and is otherwise
//! recorded first (no entry, no change) and then passed on. Someone who is
//! not an admin passes through to the route, which refuses them as before.
//! A key from the config file, which works only on the server itself, needs
//! no step-up but is audited the same (see [`stepped_up`]).
//!
//! The server's kill switch (`POST /v1/kill-switch`) follows the Kill switch
//! rule: pressing (`engaged` true or absent) needs no step-up and is recorded
//! after it holds; releasing needs one and is recorded first.

use super::mount::{audited, record, Audit};
use super::{admin_of, guard, Admin};
use crate::kernel::Kernel;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;

/// Each admin change the app's routes outside `/v1/admin` make: its method,
/// its path (`*` is one segment) and the audit action it is recorded under.
/// Bodies are not recorded: several carry keys and secrets.
pub const ADMIN_CHANGES: &[(&str, &str, &str)] = &[
    ("PUT", "/v1/secrets/*", "secret_set"),
    ("POST", "/v1/backups", "backup_made"),
    ("PUT", "/v1/model", "default_model_set"),
    ("PUT", "/v1/adapters/*", "provider_set"),
    ("POST", "/v1/adapters/*/sign-in", "provider_sign_in_started"),
    ("POST", "/v1/adapters/*/sign-in/complete", "provider_signed_in"),
    ("PUT", "/v1/registrations/*", "registration_set"),
    ("POST", "/v1/connectors", "connector_added"),
    ("DELETE", "/v1/connectors/*", "connector_removed"),
    ("PUT", "/v1/connectors/*/tools/*", "connector_tool_set"),
    ("PUT", "/v1/people/*/roles", "role_changed"),
    ("PUT", "/v1/data-access", "data_access_set"),
    ("DELETE", "/v1/people/*/account", "account_unbound"),
    ("POST", "/v1/updates/check", "update_installed"),
    ("POST", "/v1/updates/rollback", "update_rolled_back"),
];

/// The app's own route to the server's kill switch.
pub const KILL_SWITCH_ROUTE: &str = "/v1/kill-switch";

/// The most of a kill switch request's body read to see whether it presses
/// or releases: far above `{"engaged": false}`.
pub const KILL_BODY_MAX_BYTES: usize = 64 * 1024;

fn matches(pattern: &str, path: &str) -> bool {
    let (p, q): (Vec<&str>, Vec<&str>) = (pattern.split('/').collect(), path.trim_end_matches('/').split('/').collect());
    p.len() == q.len() && p.iter().zip(&q).all(|(a, b)| *a == "*" && !b.is_empty() || a == b)
}

/// The audit action of an admin change at `method` `path`, when it is one.
pub fn action_of(method: &str, path: &str) -> Option<&'static str> {
    // Match the same decoded, slash-trimmed wildcard as put_policy.
    if method == "PUT" {
        if let Some(layer) = path.strip_prefix("/v1/policies/") {
            if let Ok(layer) = percent_encoding::percent_decode_str(layer).decode_utf8() {
                let layer = layer.trim_matches('/');
                if layer == "org" || layer.starts_with("group/") {
                    return Some("policy_set");
                }
            }
        }
    }
    ADMIN_CHANGES.iter().find(|(m, p, _)| *m == method && matches(p, path)).map(|(_, _, a)| *a)
}

fn step_up_refused() -> Response {
    (StatusCode::FORBIDDEN, Json(json!({ "error": guard::STEP_UP_ERROR, "message": "Confirm your password to make this change." }))).into_response()
}

/// Whether the request may make an admin change now: its session stepped up
/// within the window, or it carries a key from the server's config file.
/// Such a key works only on the server itself over its local address
/// (`api::config_tokens_on_this_machine`), never from the internet the
/// step-up guards against, and holding it already takes the config file.
fn stepped_up(k: &Kernel, req: &Request) -> bool {
    let cred = crate::api::credential(req.headers());
    cred.and_then(guard::stepped_up_until).is_some() || cred.is_some_and(|t| k.cfg.person_by_token(t).is_some())
}

/// The layer: see the module documentation.
pub(crate) async fn gate(State(k): State<Arc<Kernel>>, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let kill = req.method() == Method::POST && path == KILL_SWITCH_ROUTE;
    let action = action_of(req.method().as_str(), &path);
    if action.is_none() && !kill {
        return next.run(req).await;
    }
    let Ok(person) = crate::api::who(&k, req.headers()).await else { return next.run(req).await };
    if !admin_of(&person, k.accounts.role(&person.id).await) {
        return next.run(req).await;
    }
    let source = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip().to_string());
    let a = Admin { person, source };
    if kill {
        return kill_switch(&k, &a, req, next).await;
    }
    if !stepped_up(&k, &req) {
        return step_up_refused();
    }
    let audit = Audit { action: action.unwrap_or_default().into(), target: Some(path), ..Default::default() };
    audited(&k, &a, audit, next.run(req)).await
}

/// `POST /v1/kill-switch` from an admin: a press at once, a release after
/// a step-up, each with the Admin section's audit entry.
async fn kill_switch(k: &Arc<Kernel>, a: &Admin, req: Request, next: Next) -> Response {
    let (parts, body) = req.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, KILL_BODY_MAX_BYTES).await else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "the body is too large" }))).into_response();
    };
    let engaged = serde_json::from_slice::<Value>(&bytes).ok().and_then(|v| v["engaged"].as_bool()).unwrap_or(true);
    let req = Request::from_parts(parts, Body::from(bytes));
    let target = Some("server".to_string());
    if engaged {
        let r = next.run(req).await;
        if r.status().is_success() {
            if let Err(why) = record(k, a, &Audit { action: "kill_switch_engaged".into(), target, ..Default::default() }).await {
                tracing::error!("the kill switch is on, but its audit entry could not be written: {why}");
            }
        }
        return r;
    }
    if !stepped_up(k, &req) {
        return step_up_refused();
    }
    let audit = Audit { action: "kill_switch_released".into(), target, before: Some(json!({ "engaged": true })), after: Some(json!({ "engaged": false })) };
    audited(k, a, audit, next.run(req)).await
}

#[cfg(test)]
mod tests {
    use super::action_of;

    /// Every owner route outside the Admin section that changes what an admin
    /// administers is gated, and a person's own settings are not.
    #[test]
    fn admin_changes_are_known_and_personal_ones_are_not() {
        for (m, p) in [
            ("PUT", "/v1/policies/org"),
            ("PUT", "/v1/policies/%6frg"),
            ("PUT", "/v1/policies//org/"),
            ("PUT", "/v1/policies/group/staff"),
            ("PUT", "/v1/policies/group%2Fstaff"),
            ("PUT", "/v1/adapters/claude"),
            ("POST", "/v1/adapters/claude/sign-in"),
            ("POST", "/v1/adapters/claude/sign-in/complete"),
            ("PUT", "/v1/model"),
            ("PUT", "/v1/registrations/google"),
            ("POST", "/v1/connectors"),
            ("DELETE", "/v1/connectors/github"),
            ("PUT", "/v1/connectors/github/tools/create_issue"),
            ("PUT", "/v1/people/mo/roles"),
            ("PUT", "/v1/data-access"),
            ("DELETE", "/v1/people/mo/account"),
            ("POST", "/v1/updates/check"),
            ("POST", "/v1/updates/rollback"),
            ("PUT", "/v1/secrets/mail_token"),
            ("POST", "/v1/backups"),
        ] {
            assert!(action_of(m, p).is_some(), "{m} {p} changes an admin's settings with no step-up");
        }
        for (m, p) in [
            ("PUT", "/v1/policies/user/alice"),
            ("PUT", "/v1/policies/task/t1"),
            ("GET", "/v1/policies/org"),
            ("GET", "/v1/model"),
            ("GET", "/v1/adapters"),
            ("DELETE", "/v1/connectors/github/connection"),
            ("POST", "/v1/connectors/github/sign-in"),
            ("PUT", "/v1/preferences"),
            ("POST", "/v1/adapters/claude/conformance"),
            ("PUT", "/v1/people//roles"),
        ] {
            assert_eq!(action_of(m, p), None, "{m} {p} is a person's own, or a read");
        }
    }
}
