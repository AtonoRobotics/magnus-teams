//! The Admin section's guard: the signed-in person must be an owner or admin,
//! and a change needs a step-up no older than [`ADMIN_STEP_UP_SECONDS`]
//! (spec rev 141, Administration). See the module documentation in `mod.rs`.

use super::Admin;
use crate::accounts::{Accounts, Role, SignInError};
use crate::api::{credential, err, internal, on_this_machine, over_tls, who, Transport};
use crate::config::Person;
use crate::kernel::Kernel;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, LazyLock, Mutex};

/// How long a confirmed password (and code) lets an admin make changes
/// without being asked again: 5 minutes, as the spec says.
pub const ADMIN_STEP_UP_SECONDS: i64 = 300;

/// Where the kill switches live. Pressing one (`<here>/.../press`) needs no
/// step-up; everything else under it does.
pub const KILL_SWITCH_PATH: &str = "/v1/admin/kill-switch";

/// The error a change answers with when its session has not stepped up.
pub const STEP_UP_ERROR: &str = "step_up";

/// When each session last confirmed its password, by the SHA-256 of its
/// credential (the token itself is never kept). Process-wide, because the
/// plain and TLS listeners each build their own router and a session may
/// reach either; a restart forgets it, which only asks again.
static STEPPED_UP: LazyLock<Mutex<HashMap<[u8; 32], DateTime<Utc>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn key(credential: &str) -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(credential.as_bytes()).into()
}

fn window() -> chrono::Duration {
    chrono::Duration::seconds(ADMIN_STEP_UP_SECONDS)
}

/// Whether a step-up made `at` still holds `now`.
pub fn fresh(at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    at <= now && now - at < window()
}

/// Record that the session holding `credential` confirmed its password `at`,
/// and forget every step-up that has lapsed.
pub(crate) fn mark(credential: &str, at: DateTime<Utc>) {
    let mut m = STEPPED_UP.lock().unwrap_or_else(|e| e.into_inner());
    m.retain(|_, t| fresh(*t, at));
    m.insert(key(credential), at);
}

/// Until when the session holding `credential` may make changes without
/// confirming again, when its step-up still holds.
pub fn stepped_up_until(credential: &str) -> Option<DateTime<Utc>> {
    let at = *STEPPED_UP.lock().unwrap_or_else(|e| e.into_inner()).get(&key(credential))?;
    fresh(at, Utc::now()).then(|| at + window())
}

/// Whether `p` administers here: the config's owner or admin role, or the
/// server's own directory's admin role.
pub fn admin_of(p: &Person, role: Role) -> bool {
    p.is_owner() || p.has_role("admin") || role == Role::Admin
}

/// Whether a request needs a fresh step-up: any change, except pressing a
/// kill switch and the step-up itself.
pub fn needs_step_up(method: &Method, path: &str) -> bool {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return false;
    }
    let press = path.starts_with(KILL_SWITCH_PATH) && path.ends_with("/press");
    !(press || path == "/v1/admin/step-up")
}

/// The guard on every admin route: signed in, an admin, and stepped up for a
/// change. It hands the handler an [`Admin`].
pub(crate) async fn guard(State(k): State<Arc<Kernel>>, mut req: Request, next: Next) -> Response {
    let h = req.headers();
    let p = match who(&k, h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if !admin_of(&p, k.accounts.role(&p.id).await) {
        return err(StatusCode::FORBIDDEN, "this needs an owner or admin");
    }
    if needs_step_up(req.method(), req.uri().path()) && credential(h).and_then(stepped_up_until).is_none() {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": STEP_UP_ERROR, "message": "Confirm your password to make this change." }))).into_response();
    }
    let source = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip().to_string());
    req.extensions_mut().insert(Admin { person: p, source });
    next.run(req).await
}

/// What checking a step-up's password found.
enum Confirmed {
    Yes,
    NeedsCode,
    Wrong,
    Locked(DateTime<Utc>),
    Unavailable(String),
}

/// `POST /v1/admin/step-up {password, code?}`: confirm the admin's password,
/// and their code when they use one, as sign-in checks them (a wrong one
/// counts toward the same lockout), and let this session make changes for
/// [`ADMIN_STEP_UP_SECONDS`].
pub(crate) async fn step_up(
    State(k): State<Arc<Kernel>>,
    Extension(t): Extension<Transport>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(a): Extension<Admin>,
    h: HeaderMap,
    Json(b): Json<Value>,
) -> Response {
    if let Err(r) = over_tls(&k, t, peer) {
        return r;
    }
    let Some(cred) = credential(&h) else { return err(StatusCode::UNAUTHORIZED, "sign in") };
    let password = b["password"].as_str().unwrap_or_default();
    let code = b["code"].as_str().map(str::trim).filter(|c| !c.is_empty());
    let counter = if on_this_machine(peer, &h) { Accounts::local_counter(&a.person.id) } else { Accounts::network_counter(&a.person.id) };
    let refuse = |e: &str, m: &str| (StatusCode::FORBIDDEN, Json(json!({ "error": e, "message": m }))).into_response();
    match confirm(&k, &a.person.id, password, code, &counter).await {
        Ok(Confirmed::Yes) => {
            let now = Utc::now();
            mark(cred, now);
            Json(json!({ "stepped_up_until": now + window() })).into_response()
        }
        Ok(Confirmed::NeedsCode) => refuse("code_needed", "Enter the code from your authenticator app."),
        Ok(Confirmed::Wrong) => refuse("wrong", "The password or code is wrong."),
        Ok(Confirmed::Locked(until)) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "locked", "message": format!("Too many wrong passwords. Try again after {}.", until.format("%H:%M UTC")), "retry_at": until })),
        )
            .into_response(),
        Ok(Confirmed::Unavailable(m)) => (StatusCode::BAD_GATEWAY, Json(json!({ "error": "unavailable", "message": m }))).into_response(),
        Err(e) => internal(e),
    }
}

/// Check `person`'s password and code where sign-in checks them: the
/// identity provider for a person it keeps, else the server's own store.
async fn confirm(k: &Kernel, person: &str, password: &str, code: Option<&str>, counter: &str) -> anyhow::Result<Confirmed> {
    let local_only = k.cfg.person(person).is_none() && k.accounts.is_local_fresh(person).await;
    if let Some(id) = k.identity().filter(|_| !local_only) {
        if let Some(until) = k.accounts.locked(counter) {
            return Ok(Confirmed::Locked(until));
        }
        // Counted before the provider answers, as sign-in does, so requests
        // sent at once cannot all pass the lockout first.
        k.accounts.failed(counter);
        return Ok(match id.password(person, password, code, true).await {
            Ok(crate::identity::Polled::Done { person: who, .. }) if who == person => {
                k.accounts.succeeded(counter);
                Confirmed::Yes
            }
            Ok(crate::identity::Polled::Refused(_) | crate::identity::Polled::Done { .. }) => Confirmed::Wrong,
            Ok(crate::identity::Polled::NeedsCode) => {
                k.accounts.forgive(counter);
                Confirmed::NeedsCode
            }
            Ok(_) | Err(_) => {
                k.accounts.forgive(counter);
                Confirmed::Unavailable(format!("{} could not confirm your password. Try again in a few minutes.", id.label()))
            }
        });
    }
    Ok(match k.accounts.sign_in_counted_with_code(person, password, code, counter).await? {
        Ok(()) => Confirmed::Yes,
        Err(SignInError::NeedsCode) => Confirmed::NeedsCode,
        Err(SignInError::Locked(until)) => Confirmed::Locked(until),
        Err(SignInError::Rejected | SignInError::MustChoose) => Confirmed::Wrong,
    })
}
