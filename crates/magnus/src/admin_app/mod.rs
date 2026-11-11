//! Administration in the app (spec rev 141, Administration): an owner or
//! admin administers Magnus from an Admin section in the same app the team
//! uses, on desktop and in the browser. This module is that section's
//! server side; the retired VPN-only console (`crate::admin`) is replaced by
//! it, section by section.
//!
//! # Who is an admin
//! A signed-in person is an admin here when any of these holds (`admin_of`):
//! - the config gives them the `owner` role (`Person::is_owner`);
//! - the config gives them the `admin` role (`Person::has_role("admin")`);
//! - the server's own directory records them as an admin
//!   (`accounts::Role::Admin`, what the console's People screen sets).
//!
//! # Step-up
//! The server is reachable from the internet, so every admin change asks for
//! the admin's password (and code, when they use one) again unless this
//! session confirmed them within [`guard::ADMIN_STEP_UP_SECONDS`]. A change is
//! any request but GET, HEAD and OPTIONS. Refused, it answers
//! `403 {"error":"step_up"}`; the app then shows its step-up sheet, posts
//! `POST /v1/admin/step-up {password, code?}` and retries. Pressing a kill
//! switch never needs it, because stopping is always safe: any path under
//! `/v1/admin/kill-switch` that ends in `/press` is exempt. Releasing one
//! (`.../release`) is a change like any other.
//!
//! # Adding a route (for the sections built on this)
//! Admin routes go in [`admin_routes`] (or a `Router` merged into it from a
//! new module): the guard has already checked the person and the step-up, and
//! puts an [`Admin`] in the request's extensions. To reuse a console route of
//! the control socket (`crate::control::routes`), mount it with
//! [`mount::pass`], which calls it in-process with `actor` set to the admin
//! (never what the client sent) and writes the console's audit entry first:
//!
//! ```ignore
//! .route("/v1/admin/people/{id}/disable", post(mount::pass(Some("user_disabled"))))
//! .route("/v1/admin/people", get(mount::pass(None)).post(mount::pass(Some("user_invited"))))
//! ```
//!
//! `pass` sends the request to the control route at the same path and query.
//! When the app's path differs from the control one, or the audit entry needs
//! its own target or before and after, call [`mount::control`] and
//! [`mount::audited`] from a handler, as [`kill`] does.

pub mod agents;
pub mod connectors;
pub mod guard;
pub mod invite_page;
pub mod kill;
pub mod mount;
pub mod operations;
pub mod overview;
pub mod overview_stops;
pub mod people;
pub mod remove;
pub mod roles;
pub mod rules;
#[cfg(test)]
mod sections_tests;
pub mod security;
pub mod settings;
#[cfg(test)]
mod spec_tests;
#[cfg(test)]
pub(crate) mod tests;

use crate::config::Person;
use crate::kernel::Kernel;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use std::sync::Arc;

pub use guard::{admin_of, ADMIN_STEP_UP_SECONDS};

/// The signed-in admin a guarded request is made by, as the guard found them.
#[derive(Clone, Debug)]
pub struct Admin {
    pub person: Person,
    /// The address the request came from, for the audit entry.
    pub source: Option<String>,
}

impl Admin {
    /// Who the log names as doing it: `admin:<id>`, the form the console's
    /// changes take (its `sub` is the person's id since #508), so the control
    /// routes and whatever reads their events treat both alike.
    pub fn actor(&self) -> String {
        format!("admin:{}", self.person.id)
    }

    /// Their name as the audit shows it.
    pub fn name(&self) -> String {
        if self.person.name.is_empty() {
            self.person.id.clone()
        } else {
            self.person.name.clone()
        }
    }
}

/// Every route of the Admin section, the guarded and the few every signed-in
/// person reaches (`standing`, `stops`). Merged into the app's API.
pub fn router(k: Arc<Kernel>) -> Router<Arc<Kernel>> {
    Router::new().route("/v1/admin/standing", get(standing)).route("/v1/stops", get(kill::stops)).merge(invite_page::routes()).merge(guarded(admin_routes(), k))
}

/// `routes` behind the guard: each answers only an admin, a change only after
/// a step-up, and each handler finds the [`Admin`] in its extensions.
pub fn guarded(routes: Router<Arc<Kernel>>, k: Arc<Kernel>) -> Router<Arc<Kernel>> {
    routes.route_layer(axum::middleware::from_fn_with_state(k, guard::guard))
}

/// The routes only an admin reaches, put behind the guard by [`router`].
/// A section adds its own here: `.merge(crate::admin_app::people::routes())`.
fn admin_routes() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/admin/step-up", post(guard::step_up))
        .merge(kill::routes())
        .merge(overview::routes())
        .merge(people::routes())
        .merge(connectors::routes())
        .merge(rules::routes())
        .merge(security::routes())
        .merge(operations::routes())
        .merge(crate::spend::routes::admin_routes())
        .merge(agents::routes())
}

/// Whether the signed-in person is an admin, so the app shows the Admin
/// entry, and until when their step-up holds. Any signed-in person may ask.
async fn standing(State(k): State<Arc<Kernel>>, h: HeaderMap) -> Response {
    let p = match crate::api::who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let admin = admin_of(&p, k.accounts.role(&p.id).await);
    let until = crate::api::credential(&h).and_then(guard::stepped_up_until);
    Json(json!({ "admin": admin, "owner": p.is_owner(), "step_up_seconds": ADMIN_STEP_UP_SECONDS, "stepped_up_until": if admin { until } else { None } })).into_response()
}
