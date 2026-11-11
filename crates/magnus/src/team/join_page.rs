//! The page behind a workspace's invite link (spec rev 142, Team shared
//! workspace: "Members are invited by email or link"). Someone with no
//! account here makes one, name, email and password, and joins the
//! workspace with the role the link carries, within its lifetime; they are
//! then signed in. Someone with an account opens the link in Magnus.
//!
//! As safe as the `/invite` page: over TLS from the network, and every
//! unknown or ended code is counted against the address it came from, as a
//! wrong password is at sign-in, so codes cannot be guessed at speed. The
//! account is a server member, or a guest for a guest's link, never more.
use super::invites::{live, new_account};
use super::{Change, Role};
use crate::admin_app::invite_page::{escape, shell, signed_in};
use crate::api::Transport;
use crate::events::Ev;
use crate::kernel::Kernel;
use axum::extract::{ConnectInfo, Form, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

/// Where a workspace's invite link points, below the server's address.
pub const JOIN_PATH: &str = "/join";

pub(super) fn routes() -> Router<Arc<Kernel>> {
    Router::new().route(JOIN_PATH, get(page).post(create))
}

/// The link's path, below the server's address.
pub fn join_path(code: &str) -> String {
    format!("{JOIN_PATH}?code={code}")
}

fn form(code: &str, workspace: &str, error: Option<&str>) -> Response {
    let error = error.map(|e| format!("<p class=error>{}</p>", escape(e))).unwrap_or_default();
    let (code, workspace) = (escape(code), escape(workspace));
    shell(
        "Join Magnus",
        &format!(
            "<h1>Join {workspace} in Magnus</h1><p>Make your account to join. You sign in with your email address and the password you choose.</p>{error}<form method=post action={JOIN_PATH}><input type=hidden name=code value=\"{code}\">{}<button>Join</button></form><p>Already have an account here? Sign in to Magnus and open <a href=\"/#/join/{code}\">this invitation</a> there.</p>",
            crate::invitations::signup_fields()
        ),
    )
}

fn ended() -> Response {
    shell("Invitation", "<h1>This invitation has ended.</h1><p>Ask the person who invited you for a new link.</p>")
}

fn too_many(until: chrono::DateTime<chrono::Utc>) -> Response {
    let mut r = shell("Invitation", &format!("<h1>Too many tries.</h1><p>Try again after {}.</p>", until.format("%H:%M UTC")));
    *r.status_mut() = StatusCode::TOO_MANY_REQUESTS;
    r
}

/// The live link invite for a code: one bound to an address goes by email,
/// never through this page.
fn link_invite(k: &Kernel, code: &str) -> Option<(String, String, super::chat::Invite)> {
    live(k, code).filter(|(_, _, i)| i.email.is_none())
}

async fn page(State(k): State<Arc<Kernel>>, Extension(t): Extension<Transport>, ConnectInfo(peer): ConnectInfo<SocketAddr>, Query(q): Query<HashMap<String, String>>) -> Response {
    if let Err(r) = crate::api::over_tls(&k, t, peer) {
        return r;
    }
    let tries = crate::invitations::attempts(peer.ip());
    if let Some(until) = k.accounts.locked(&tries) {
        return too_many(until);
    }
    let code = q.get("code").map(|s| s.trim().to_string()).unwrap_or_default();
    match link_invite(&k, &code) {
        Some((_, name, _)) => form(&code, &name, None),
        None => {
            k.accounts.failed(&tries);
            ended()
        }
    }
}

async fn create(State(k): State<Arc<Kernel>>, Extension(t): Extension<Transport>, ConnectInfo(peer): ConnectInfo<SocketAddr>, Form(f): Form<HashMap<String, String>>) -> Response {
    if let Err(r) = crate::api::over_tls(&k, t, peer) {
        return r;
    }
    let tries = crate::invitations::attempts(peer.ip());
    if let Some(until) = k.accounts.locked(&tries) {
        return too_many(until);
    }
    let code = f.get("code").map(|s| s.trim().to_string()).unwrap_or_default();
    let Some((ws, workspace, inv)) = link_invite(&k, &code) else {
        k.accounts.failed(&tries);
        return ended();
    };
    let again = |why: &str| form(&code, &workspace, Some(why));
    let who = match crate::invitations::signup(&k, &f) {
        Ok(who) => who,
        Err(why) => return again(&why),
    };
    let (email, name, password) = (who.email, who.name, who.password);
    let role = inv.role.unwrap_or(Role::Member);
    let (id, token) = match new_account(&k, &format!("person:{}", inv.by), &email, &name, role == Role::Guest).await {
        Ok(made) => made,
        Err((_, message)) => {
            tracing::error!("an account from a workspace link: {message}");
            return again("Something went wrong. Try again in a minute.");
        }
    };
    match k.accounts.accept(&token, &password).await {
        Ok(Some(_)) => {}
        Ok(None) | Err(_) => return again("Something went wrong. Try again in a minute."),
    }
    if let Err(e) = k.emit(&format!("person:{id}"), Ev::Team(Change::Member { workspace: ws.clone(), person: id.clone(), role })).await {
        tracing::error!("{id} joining {ws} by link: {e:#}");
        return again("Something went wrong. Try again in a minute.");
    }
    signed_in(&k, t, &id, Some(&ws)).await.into_response()
}
