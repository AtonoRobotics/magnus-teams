//! The page behind an invitation link (spec rev 141, People: "Invite by
//! email or link"): the person chooses their password with the single-use
//! token the link carries, joins the workspaces it was invited to, and is
//! signed in here. The token is checked by `Accounts::accept`, as the app's
//! own `/v1/invites/accept` checks it.

use crate::api::Transport;
use crate::kernel::Kernel;
use axum::extract::{ConnectInfo, Form, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

/// Where an invitation link points, below the server's address.
pub const INVITE_PATH: &str = "/invite";

pub(super) fn routes() -> Router<Arc<Kernel>> {
    Router::new().route(INVITE_PATH, get(page).post(accept))
}

pub(crate) fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub(crate) fn shell(title: &str, body: &str) -> Response {
    let html = format!(
        "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>{title}</title><style>body{{font-family:system-ui,sans-serif;max-width:28em;margin:4em auto;padding:0 1em;color:#1a1a1a;background:#fff}}input,button{{font:inherit}}input{{display:block;width:100%;box-sizing:border-box;margin:.4em 0 1em;padding:.5em}}button{{padding:.5em 1.2em}}.error{{color:#a00}}@media (prefers-color-scheme:dark){{body{{background:#161616;color:#eee}}}}</style></head><body>{body}</body></html>"
    );
    (StatusCode::OK, [("content-type", "text/html; charset=utf-8")], html).into_response()
}

fn form(token: &str, error: Option<&str>) -> Response {
    let error = error.map(|e| format!("<p class=error>{}</p>", escape(e))).unwrap_or_default();
    shell(
        "Join Magnus",
        &format!(
            "<h1>Join Magnus</h1><p>Choose the password you will sign in with.</p>{error}<form method=post action={INVITE_PATH}><input type=hidden name=token value=\"{}\"><label>Password<input type=password name=password autocomplete=new-password required></label><label>Repeat it<input type=password name=confirm autocomplete=new-password required></label><button>Join</button></form>",
            escape(token)
        ),
    )
}

async fn page(State(k): State<Arc<Kernel>>, Extension(t): Extension<Transport>, ConnectInfo(peer): ConnectInfo<SocketAddr>, Query(q): Query<HashMap<String, String>>) -> Response {
    if let Err(r) = crate::api::over_tls(&k, t, peer) {
        return r;
    }
    if let Some(code) = q.get("link") {
        return crate::account_links::page(&k, peer, code);
    }
    let token = q.get("token").map(|s| s.trim().to_string()).unwrap_or_default();
    if token.is_empty() {
        return shell("Invitation", "<h1>This link has no invitation in it.</h1><p>Ask the person who invited you to send it again.</p>");
    }
    form(&token, None)
}

async fn accept(State(k): State<Arc<Kernel>>, Extension(t): Extension<Transport>, ConnectInfo(peer): ConnectInfo<SocketAddr>, Form(f): Form<HashMap<String, String>>) -> Response {
    if let Err(r) = crate::api::over_tls(&k, t, peer) {
        return r;
    }
    if f.contains_key("link") {
        return crate::account_links::create(&k, t, peer, &f).await;
    }
    let token = f.get("token").map(|s| s.trim().to_string()).unwrap_or_default();
    let password = f.get("password").cloned().unwrap_or_default();
    if f.get("confirm") != Some(&password) {
        return form(&token, Some("The two passwords do not match."));
    }
    if password.chars().count() < k.cfg.sign_in.min_password_length {
        return form(&token, Some(&format!("A password needs at least {} characters.", k.cfg.sign_in.min_password_length)));
    }
    match k.accept_invitation(&token, &password).await {
        Ok(Some(a)) => signed_in(&k, t, &a.person, a.workspaces.first().map(String::as_str)).await,
        Ok(None) => shell("Invitation", "<h1>This invitation is spent or expired.</h1><p>Ask the person who invited you to send it again.</p>"),
        Err(e) => {
            tracing::error!("accepting an invitation: {e:#}");
            shell("Invitation", "<h1>Something went wrong.</h1><p>Try again in a minute.</p>")
        }
    }
}

/// Sign a newly made local account in to this browser and open Magnus, in
/// the workspace they joined when there is one. Anyone else, or a session
/// that does not open, signs in as anyone does.
pub(crate) async fn signed_in(k: &Arc<Kernel>, t: Transport, person: &str, workspace: Option<&str>) -> Response {
    let fallback = || shell("You are in", "<h1>You are in.</h1><p>Open Magnus and sign in with your email address or name and the password you chose.</p>");
    if !k.accounts.is_local(person) {
        return fallback();
    }
    let issued = crate::api::issue(k, t, person, false, None).await;
    let Some(cookie) = issued.status().is_success().then(|| issued.headers().get("set-cookie").cloned()).flatten() else { return fallback() };
    let to = workspace.map(|w| format!("/#/team/{w}")).unwrap_or_else(|| "/".into());
    let mut r = axum::response::Redirect::to(&to).into_response();
    r.headers_mut().insert("set-cookie", cookie);
    r
}
