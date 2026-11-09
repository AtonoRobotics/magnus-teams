//! The console's sign-in flow, against the local store, and its guard
//! on every API route (see `auth`).

use super::auth::{Session, COOKIE};
use super::Console;
use anyhow::Result;
use axum::extract::{Form, Query, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

fn cookie_of(h: &HeaderMap, name: &str) -> Option<String> {
    h.get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|p| p.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

/// Only a path on the console itself.
fn local_path(then: Option<&String>) -> String {
    then.filter(|t| t.starts_with('/') && !t.starts_with("//") && !t.contains('\\')).cloned().unwrap_or_else(|| "/".into())
}

fn page(status: StatusCode, title: &str, said: &str) -> Response {
    let body = format!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width, initial-scale=1\"><title>{title}</title><link rel=stylesheet href=/assets/app.css><link rel=stylesheet href=/assets/admin.css><script src=/assets/theme.js></script><main class=\"ad-main\"><section class=\"card ad-sign-in\"><h1>{title}</h1><p class=muted>{said}</p><div class=row><a class=\"button primary\" href=\"/auth/local\">Sign in again</a></div></section></main>"
    );
    (status, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>()
}

/// The login form's CSRF binding: the GET sets it as a cookie and embeds
/// it in the form, and the POST takes it once. A cross-origin attacker
/// reads neither, so their post cannot ride a victim's browser (#456).
/// Origin headers cannot bind instead: under the console's no-referrer
/// policy browsers submit with `Origin: null`.
const LOCAL_LOGIN_COOKIE: &str = "__Host-magnus_local";

/// Mint a login binding: the token and the cookie that carries it back.
/// None when too many logins are waiting. The path is `/`: browsers
/// reject `__Host-` cookies with any narrower path (#456).
fn mint_login(c: &Console) -> Option<(String, String)> {
    let token = crate::connectors::random();
    c.store.add_login_token(token.clone()).ok()?;
    Some((token.clone(), format!("{LOCAL_LOGIN_COOKIE}={token}; Path=/; Secure; HttpOnly; SameSite=Strict")))
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The local sign-in form: username and password, and the authenticator
/// code where the account uses one. `error` says what went wrong, if
/// anything did yet. `login_token` binds the submission to this rendering.
fn local_form(action: &str, then: &str, person: &str, name: Option<&str>, code_needed: bool, error: Option<&str>, login_token: &str) -> Response {
    let who = match name {
        // A step-up names its admin and asks only for the secrets.
        Some(n) => format!("<p class=muted>Signed in as <strong>{}</strong>. Confirm it&rsquo;s you.</p>", escape(n)),
        None => format!("<label>Username<input name=person value=\"{}\" autocomplete=username required></label>", escape(person)),
    };
    let code = if code_needed {
        "<label>6-digit code<input name=code inputmode=numeric autocomplete=one-time-code required></label>"
    } else {
        "<label>6-digit code, if your account uses one<input name=code inputmode=numeric autocomplete=one-time-code></label>"
    };
    let error = error.map(|e| format!("<p class=error>{}</p>", escape(e))).unwrap_or_default();
    let body = format!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width, initial-scale=1\"><title>Sign in to the console</title><link rel=stylesheet href=/assets/app.css><link rel=stylesheet href=/assets/admin.css><script src=/assets/theme.js></script><main class=\"ad-main\"><section class=\"card ad-sign-in\"><h1>Sign in to the console</h1>{error}<form method=post action=\"{action}\"><input type=hidden name=then value=\"{}\"><input type=hidden name=login_token value=\"{}\">{who}<label>Password<input type=password name=password autocomplete=current-password required></label>{code}<div class=row><button class=\"button primary\" type=submit>Sign in</button></div></form></section></main>",
        escape(then),
        escape(login_token)
    );
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
}

/// Render a local form with a fresh binding, refusing the submission when
/// no binding is left to mint.
fn bound_form(c: &Console, render: impl FnOnce(&str) -> Response) -> Response {
    match mint_login(c) {
        Some((token, set)) => {
            let mut r = render(&token).into_response();
            r.headers_mut().append(header::SET_COOKIE, set.parse().unwrap());
            r
        }
        None => page(StatusCode::TOO_MANY_REQUESTS, "Try again shortly", "Too many sign-ins are waiting; try again in a few minutes."),
    }
}

/// Sign in against the local store: the console's only sign-in (#505).
pub async fn local_login(State(c): State<Arc<Console>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let empty = String::new();
    let (person, then) = (q.get("person").unwrap_or(&empty).clone(), local_path(q.get("then")));
    bound_form(&c, |token| local_form("/auth/local", &then, &person, None, false, None, token))
}

#[derive(Deserialize)]
pub struct LocalLogin {
    pub person: String,
    pub password: String,
    pub code: Option<String>,
    pub then: Option<String>,
    pub login_token: Option<String>,
}

fn session_cookie(sid: &str) -> String {
    format!("{COOKIE}={sid}; Path=/; Secure; HttpOnly; SameSite=Strict")
}

/// Take a submission's login binding: the cookie and the form agree, and
/// the token is live. Checked before credentials, so a forged post
/// neither signs in nor burns the account's lockout.
fn take_binding(c: &Console, h: &HeaderMap, token: Option<&str>) -> bool {
    let cookie = cookie_of(h, LOCAL_LOGIN_COOKIE);
    cookie.as_deref() == token && token.is_some_and(|t| c.store.take_login_token(t))
}

pub async fn local_login_post(
    State(c): State<Arc<Console>>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    h: HeaderMap,
    Form(f): Form<LocalLogin>,
) -> Response {
    let person = f.person.trim().to_lowercase();
    let then = local_path(f.then.as_ref());
    let retry = |person: &str, code_needed: bool, error: &str| {
        let (person, then, error) = (person.to_string(), then.clone(), error.to_string());
        bound_form(&c, move |token| local_form("/auth/local", &then, &person, None, code_needed, Some(&error), token))
    };
    if !take_binding(&c, &h, f.login_token.as_deref()) {
        tracing::warn!("admin console: local sign-in for {person} refused: no live login binding");
        return retry(&person, false, "This sign-in expired. Start again from the console's sign-in page.");
    }
    let v = match c.kernel.call(Method::POST, "/v1/admin/console-sign-in", Some(&json!({ "person": person, "password": f.password, "code": f.code }))).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("admin console: local sign-in check: {e:#}");
            return retry(&person, false, "Sign-in is unavailable. Try again in a moment.");
        }
    };
    match v["result"].as_str() {
        Some("ok") => {
            let name = v["name"].as_str().unwrap_or(&person).to_string();
            if let Err(e) = c.audit((&person, &name), "sign_in", None, peer, None, None).await {
                tracing::error!("admin console: {e:#}");
                return retry(&person, false, "The console could not record this sign-in, so it did not let it through. Try again in a moment.");
            }
            let now = Utc::now();
            let sid = crate::connectors::random();
            // A password just typed counts as stepped up, as with a fresh
            // provider sign-in.
            let s = Session { sub: person, name, csrf: crate::connectors::random(), started: now, seen: now, refreshed: now, stepped_up: Some(now) };
            c.store.sessions.lock().unwrap().insert(sid.clone(), s);
            let mut r = Redirect::to(&then).into_response();
            r.headers_mut().append(header::SET_COOKIE, session_cookie(&sid).parse().unwrap());
            r.headers_mut().append(header::SET_COOKIE, format!("{LOCAL_LOGIN_COOKIE}=; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age=0").parse().unwrap());
            r
        }
        Some("needs_code") => retry(&person, true, "Enter the 6-digit code from your authenticator app."),
        Some("locked") => {
            tracing::warn!("admin console: local sign-in locked for {person}");
            retry(&person, false, "Too many wrong passwords. Try again in a few minutes.")
        }
        _ => {
            tracing::warn!("admin console: local sign-in refused for {person}");
            // A refusal is on the record, as a provider refusal was: who
            // tried, from where, and that the door stayed shut. A lost
            // audit entry must not open it, nor hide the refusal.
            if let Err(e) = c.audit((&person, &person), "sign_in_refused", None, peer, None, None).await {
                tracing::error!("admin console: {e:#}");
            }
            retry(&person, false, "The name, password or code is wrong.")
        }
    }
}

/// Confirm a local session with its admin's password (and code): the form.
pub async fn local_step_up(State(c): State<Arc<Console>>, h: HeaderMap, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some((_, s)) = current(&c, &h) else { return Redirect::to("/auth/local").into_response() };
    let (sub, name, then) = (s.sub.clone(), s.name.clone(), local_path(q.get("then")));
    bound_form(&c, move |token| local_form("/auth/local/step-up", &then, &sub, Some(&name), false, None, token))
}

#[derive(Deserialize)]
pub struct LocalStepUp {
    pub password: String,
    pub code: Option<String>,
    pub then: Option<String>,
    pub login_token: Option<String>,
}

/// Confirm a local session: the password just typed stamps the step-up.
pub async fn local_step_up_post(
    State(c): State<Arc<Console>>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    h: HeaderMap,
    Form(f): Form<LocalStepUp>,
) -> Response {
    let Some((sid, s)) = current(&c, &h) else { return Redirect::to("/auth/local").into_response() };
    let then = local_path(f.then.as_ref());
    let render = |code_needed: bool, error: Option<&str>| {
        let (sub, name, then) = (s.sub.clone(), s.name.clone(), then.clone());
        let error = error.map(str::to_string);
        move |token: &str| local_form("/auth/local/step-up", &then, &sub, Some(&name), code_needed, error.as_deref(), token)
    };
    if !take_binding(&c, &h, f.login_token.as_deref()) {
        tracing::warn!("admin console: local step-up for {} refused: no live login binding", s.sub);
        return bound_form(&c, render(false, Some("This confirmation expired. Start again from the console.")));
    }
    let form = |code_needed: bool, error: &str| bound_form(&c, render(code_needed, Some(error)));
    let v = match c.kernel.call(Method::POST, "/v1/admin/console-sign-in", Some(&json!({ "person": s.sub, "password": f.password, "code": f.code }))).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("admin console: local step-up check: {e:#}");
            return form(false, "Confirmation is unavailable. Try again in a moment.");
        }
    };
    match v["result"].as_str() {
        Some("ok") => {
            if let Err(e) = c.audit((&s.sub, &s.name), "step_up", None, peer, None, None).await {
                tracing::error!("admin console: {e:#}");
                return form(false, "The console could not record this confirmation, so it did not let it through. Try again in a moment.");
            }
            let mut sessions = c.store.sessions.lock().unwrap();
            if let Some(s) = sessions.get_mut(&sid) {
                s.stepped_up = Some(Utc::now());
                s.refreshed = Utc::now();
            }
            Redirect::to(&then).into_response()
        }
        Some("needs_code") => form(true, "Enter the 6-digit code from your authenticator app."),
        Some("locked") => form(false, "Too many wrong passwords. Try again in a few minutes."),
        _ => form(false, "The password or code is wrong."),
    }
}

/// The session the browser's cookie names, if it is still open: neither
/// idle too long nor older than the longest a session lasts.
fn current(c: &Console, h: &HeaderMap) -> Option<(String, Session)> {
    let sid = cookie_of(h, COOKIE)?;
    let mut sessions = c.store.sessions.lock().unwrap();
    let s = sessions.get(&sid)?.clone();
    let now = Utc::now();
    if now - s.seen > Duration::minutes(c.idle_minutes.load(std::sync::atomic::Ordering::Relaxed))
        || now - s.started > Duration::hours(c.session_hours.load(std::sync::atomic::Ordering::Relaxed))
    {
        sessions.remove(&sid);
        return None;
    }
    Some((sid, s))
}

/// Where a step-up opened in its own window comes back: it closes, and the
/// console's page, which opened it, carries on with the action.
pub async fn done() -> Response {
    let body = "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width, initial-scale=1\"><title>Confirmed</title><link rel=stylesheet href=/assets/app.css><link rel=stylesheet href=/assets/admin.css><script src=/assets/theme.js></script><script src=/assets/done.js defer></script><main class=\"ad-main\"><section class=\"card ad-sign-in\"><h1>Confirmed</h1><p class=muted>You can close this window and carry on in the console.</p><div class=row><a class=\"button primary\" href=\"/\">Back to the console</a></div></section></main>";
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
}

/// Whether a session is still open, for a stream that outlives its request.
pub fn open(c: &Console, sid: &str) -> bool {
    let sessions = c.store.sessions.lock().unwrap();
    let Some(s) = sessions.get(sid) else { return false };
    let now = Utc::now();
    now - s.seen <= Duration::minutes(c.idle_minutes.load(std::sync::atomic::Ordering::Relaxed))
        && now - s.started <= Duration::hours(c.session_hours.load(std::sync::atomic::Ordering::Relaxed))
}

fn refused(status: StatusCode, error: &str, message: &str) -> Response {
    (status, Json(json!({ "error": error, "message": message }))).into_response()
}

/// Every API route but health: an open admin session, its standing
/// checked with the kernel at least every few minutes, and on a change,
/// the session's CSRF token. The admin rides along to the route.
pub async fn guard(State(c): State<Arc<Console>>, axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>, mut req: Request, next: Next) -> Response {
    let Some((sid, mut s)) = current(&c, req.headers()) else { return refused(StatusCode::UNAUTHORIZED, "sign_in_required", "Sign in to the admin console.") };
    let now = Utc::now();
    if now - s.refreshed > Duration::seconds(c.refresh_seconds) {
        // Still here, active and an admin, at the kernel.
        let still = match c.kernel.call(Method::GET, &format!("/v1/admin/console-standing/{}", urlencode(&s.sub)), None).await {
            Ok(v) => v["ok"] == true,
            Err(e) => {
                tracing::warn!("admin console: {}'s session ends: {e:#}", s.sub);
                false
            }
        };
        if !still {
            c.store.sessions.lock().unwrap().remove(&sid);
            if let Err(e) = c.audit((&s.sub, &s.name), "session_ended", None, peer, None, None).await {
                tracing::error!("admin console: {e:#}");
            }
            return refused(StatusCode::UNAUTHORIZED, "sign_in_required", "Your console session ended: your account no longer signs in as an admin.");
        }
        s.refreshed = now;
    }
    if req.method() != Method::GET && req.method() != Method::HEAD && req.headers().get("x-csrf-token").and_then(|v| v.to_str().ok()) != Some(s.csrf.as_str()) {
        return refused(StatusCode::FORBIDDEN, "csrf", "This change did not come from the console's own page.");
    }
    s.seen = now;
    c.store.sessions.lock().unwrap().insert(sid.clone(), s.clone());
    req.extensions_mut().insert(Admin { sid, session: s, step_up_seconds: c.step_up_seconds, peer });
    next.run(req).await
}

/// The signed-in admin, for a route.
#[derive(Clone)]
pub struct Admin {
    pub sid: String,
    pub session: Session,
    pub step_up_seconds: i64,
    /// Where the request came from, for the audit log.
    pub peer: std::net::SocketAddr,
}

impl Admin {
    /// Refuse an action that needs a fresh password, unless one was typed
    /// in the last five minutes.
    pub fn step_up(&self) -> Result<(), Response> {
        match self.session.stepped_up(Utc::now(), self.step_up_seconds) {
            true => Ok(()),
            false => Err(refused(StatusCode::UNAUTHORIZED, "step_up_required", "Confirm it's you: sign in again to do this.")),
        }
    }
}

/// Who is signed in, the token every change carries, and until when a
/// step-up counts.
pub async fn me(axum::Extension(a): axum::Extension<Admin>) -> Response {
    let until = a.session.stepped_up.map(|t| t + Duration::seconds(a.step_up_seconds)).filter(|t| *t > Utc::now());
    Json(json!({ "name": a.session.name, "sub": a.session.sub, "csrf": a.session.csrf, "stepped_up_until": until })).into_response()
}

/// Sign out of the console.
pub async fn logout(State(c): State<Arc<Console>>, axum::Extension(a): axum::Extension<Admin>) -> Response {
    if let Err(e) = c.audit((&a.session.sub, &a.session.name), "sign_out", None, a.peer, None, None).await {
        return refused(StatusCode::SERVICE_UNAVAILABLE, "audit", &format!("{e:#}"));
    }
    c.store.sessions.lock().unwrap().remove(&a.sid);
    ([(header::SET_COOKIE, format!("{COOKIE}=; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age=0"))], StatusCode::NO_CONTENT).into_response()
}

/// An action that needs a fresh password: refused without one in the last
/// five minutes. The first such action in the console's own sections will
/// call `Admin::step_up` the same way.
pub async fn confirm(axum::Extension(a): axum::Extension<Admin>) -> Response {
    match a.step_up() {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(r) => r,
    }
}
