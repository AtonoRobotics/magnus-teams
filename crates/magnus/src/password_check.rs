//! The one way a typed password is checked: sign-in, the Admin step-up,
//! and changing or choosing a password all come here. This settles who
//! keeps the person's password (the identity provider or the server's own
//! store) and whether they are known; `Accounts::check_password` counts the
//! attempt and asks. The answer for "too many wrong passwords" is shared
//! here too, so every endpoint says it the same way.

use crate::accounts::Keeper;
pub use crate::accounts::{Attempt, Checked, Via};
use crate::identity::Identity;
use crate::kernel::Kernel;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;

/// Where a request came from, for counting its password.
pub fn via(peer: SocketAddr, h: &HeaderMap) -> Via {
    if crate::api::on_this_machine(peer, h) {
        Via::ThisMachine
    } else {
        Via::Network
    }
}

/// The identity provider that keeps `person`'s password, or None when the
/// server's own store does. A person only the server keeps is checked here,
/// not at the provider, which never heard of them (#429); a person the
/// config knows keeps the provider, even with a local namesake, as
/// `resolve_person` resolves the config first.
pub async fn provider_for(k: &Kernel, person: &str) -> Option<Arc<dyn Identity>> {
    let id = k.identity()?;
    provider_keeps(k.cfg.person(person).is_some(), k.accounts.is_local_fresh(person).await).then_some(id)
}

fn provider_keeps(in_config: bool, local: bool) -> bool {
    in_config || !local
}

/// Check a typed password where it is kept, counting it on its counter.
pub async fn check(k: &Kernel, a: &Attempt<'_>) -> anyhow::Result<Checked> {
    let in_config = k.cfg.person(a.person).is_some();
    let local = k.accounts.is_local_fresh(a.person).await;
    let provider = k.identity().filter(|_| provider_keeps(in_config, local));
    let keeper = Keeper { provider, known: in_config || local, backup_allowed: k.may_use_password(a.person) };
    k.accounts.check_password(&keeper, a).await
}

/// What the lockout says, naming when it ends.
pub fn locked_message(until: DateTime<Utc>) -> String {
    format!("Too many wrong passwords. Try again after {}.", until.format("%H:%M UTC"))
}

/// 429 with when to try again: `{error, retry_at}`, or with `code`,
/// `{error: code, message, retry_at}` for callers that answer with codes.
pub fn locked(until: DateTime<Utc>, code: Option<&str>) -> Response {
    let body = match code {
        None => json!({ "error": locked_message(until), "retry_at": until }),
        Some(c) => json!({ "error": c, "message": locked_message(until), "retry_at": until }),
    };
    (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response()
}
