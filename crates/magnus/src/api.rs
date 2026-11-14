//! The HTTP API, for people's apps. Every request names a person: by a
//! session from sign-in (the desktop app sends it as a bearer token,
//! a browser as an HttpOnly cookie) or by a configured API token. Approvals,
//! answers and policy arrive only here, a channel the agent cannot reach (its
//! sandbox has no network and no tool calls this API).

use crate::config::{Adapter, Person};
use crate::events::Ev;
use crate::inbox::{self, Answer};
use crate::kernel::{new_id, Kernel, HARNESS_VERSION};
use crate::state::{Run, Status};
use axum::{
    extract::{ConnectInfo, Form, Path, Query, Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Extension, Json, Router,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;

type K = State<Arc<Kernel>>;

pub(crate) fn err(code: StatusCode, msg: impl Into<String>) -> Response {
    (code, Json(json!({ "error": msg.into() }))).into_response()
}

pub(crate) fn internal(e: anyhow::Error) -> Response {
    if let Some(r) = crate::halts::refused(&e) {
        return r;
    }
    if let Some(r) = crate::spend::routes::refused(&e) {
        return r;
    }
    tracing::error!("api: {e:#}");
    err(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
}

/// New work is refused while the kill switch is on, before anything about
/// it is recorded (owner's ruling 11).
pub(crate) fn refused_by_kill_switch(k: &crate::kernel::Kernel) -> Option<Response> {
    k.read(|s| s.kill_switch).then(|| internal(crate::kernel::KillSwitchOn.into()))
}

/// Which listener a request came in on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Plain,
    Tls,
}

pub const SESSION_COOKIE: &str = "magnus_session";

fn bearer(h: &HeaderMap) -> Option<&str> {
    h.get("authorization").and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "))
}

fn cookie<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    h.get_all("cookie").iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';')).filter_map(|c| c.trim().split_once('=')).find(|(n, _)| *n == name).map(|(_, v)| v)
}

pub(crate) fn credential(h: &HeaderMap) -> Option<&str> {
    bearer(h).or_else(|| cookie(h, SESSION_COOKIE))
}

pub(crate) async fn who(k: &Kernel, h: &HeaderMap) -> Result<Person, Response> {
    let token = credential(h).ok_or_else(|| err(StatusCode::UNAUTHORIZED, "sign in"))?;
    if let Some(p) = k.cfg.person_by_token(token) {
        return k.person(&p.id).ok_or_else(|| err(StatusCode::UNAUTHORIZED, "sign in again"));
    }
    k.accounts.session(token).await.and_then(|s| k.person(&s.person)).ok_or_else(|| err(StatusCode::UNAUTHORIZED, "sign in again"))
}

pub(crate) async fn developer(k: &Kernel, h: &HeaderMap) -> Result<Person, Response> {
    let p = who(k, h).await?;
    if !p.is_developer() {
        return Err(err(StatusCode::FORBIDDEN, "this needs the developer role"));
    }
    Ok(p)
}

/// A browser sends the session cookie on its own, so a change made with the
/// cookie must also carry a header another site's page cannot set. The
/// recovery, invitation and join forms are the exception: each is authorized
/// by its token or code, never the cookie, so a stale session cookie must
/// not block it (#541).
async fn cookie_guard(req: Request, next: Next) -> Response {
    let h = req.headers();
    let safe = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    let recovery = *req.method() == Method::POST && matches!(req.uri().path(), "/reset" | "/invite" | "/join");
    if !safe && !recovery && bearer(h).is_none() && cookie(h, SESSION_COOKIE).is_some() && h.get("x-magnus-client").is_none() {
        return err(StatusCode::FORBIDDEN, "missing x-magnus-client header");
    }
    next.run(req).await
}

pub(crate) async fn owner(k: &Kernel, h: &HeaderMap) -> Result<Person, Response> {
    let p = who(k, h).await?;
    if !p.is_owner() {
        return Err(err(StatusCode::FORBIDDEN, "this needs the owner role"));
    }
    Ok(p)
}

pub fn router(k: Arc<Kernel>, transport: Transport) -> Router {
    Router::new()
        .route("/", get(|| crate::ui::assets::ui_file(Path("index.html".into()))))
        .route("/{file}", get(crate::ui::assets::ui_file))
        .route("/fonts/{file}", get(crate::ui::assets::ui_font))
        .route("/v1/health", get(health))
        .route("/v1/sign-in", get(sign_in_method).post(sign_in))
        .route("/v1/devices", get(list_devices).post(enroll_device))
        .route("/v1/devices/{id}", axum::routing::delete(revoke_device))
        .route("/v1/sign-in/browser", post(browser_begin))
        .route("/v1/sign-in/browser/finish", post(browser_finish))
        .route("/v1/sign-in/device", post(device_start))
        .route("/v1/sign-in/device/{flow}", post(device_poll))
        .route("/v1/invites/accept", post(accept_invite))
        .route("/reset", get(reset_page).post(reset_confirm))
        .route("/v1/password", post(choose_password))
        .route("/v1/sign-out", post(sign_out))
        .route("/v1/tasks/{id}/live", get(live))
        .route("/v1/tasks/{id}/outputs/{out}", get(output))
        .route("/v1/transcribe", post(transcribe))
        .route("/v1/speak", post(speak))
        .route("/v1/me", get(me))
        .route("/v1/versions", get(versions))
        .route("/v1/tasks", post(submit).get(list_tasks))
        .route("/v1/chat", post(chat_post).get(chat_threads))
        .route("/v1/chat/{thread}", get(chat_thread))
        .route("/v1/chat/cards/{card}", post(chat_card))
        .route("/v1/tasks/{id}", get(task))
        .route("/v1/tasks/{id}/timeline", get(timeline))
        .route("/v1/tasks/{id}/resume", post(resume))
        .route("/v1/tasks/{id}/cancel", post(cancel))
        .route("/v1/tasks/{id}/verdict", post(verdict))
        .route("/v1/search", get(search))
        .route("/v1/inbox", get(inbox_list))
        .route("/v1/inbox/{id}", post(respond))
        .route("/v1/inbox/opened", post(inbox_opened))
        .route("/v1/home/opened", post(home_opened))
        .route("/v1/policies", get(policies))
        .route("/v1/policies/{*layer}", put(put_policy))
        .route("/v1/secrets", get(secret_names))
        .route("/v1/secrets/{name}", put(put_secret))
        .route("/v1/kill-switch", post(kill_switch).get(kill_switch_state))
        .route("/v1/backups", get(backups).post(back_up))
        .route("/v1/adapters", get(adapters))
        .route("/v1/model", get(get_model).put(put_model))
        .route("/v1/models", get(models))
        .route("/v1/adapters/{id}", put(put_adapter))
        .route("/v1/adapters/{id}/catalog", get(catalog))
        .route("/v1/adapters/{id}/conformance", post(conformance))
        .route("/v1/adapters/{id}/sign-in", post(sign_in_begin))
        .route("/v1/adapters/{id}/sign-in/complete", post(sign_in_complete))
        .route("/v1/memory", get(memory))
        .route("/v1/memory/{id}", axum::routing::delete(forget))
        .route("/v1/schedules", get(schedules).post(add_schedule))
        .route("/v1/schedules/{id}", axum::routing::delete(remove_schedule).patch(edit_schedule))
        .route("/v1/schedules/{id}/resume", post(resume_schedule))
        .route("/v1/schedules/{id}/pause", post(pause_schedule))
        .merge(crate::dev::router().merge(crate::legal::routes::router(k.clone())))
        .merge(crate::connectors::router().merge(crate::generated::router()).merge(crate::payments::router()).merge(crate::push::router()))
        .merge(crate::library::router().merge(crate::consent::router()))
        .merge(crate::update::router())
        .merge(crate::settings::router().merge(crate::passwords::router()))
        .merge(crate::relationship::router())
        .merge(crate::reactions::router())
        .merge(crate::grants::router().merge(crate::computer::router()).merge(crate::rules::router()))
        .merge(crate::commitments::router())
        .merge(crate::knowledge::router().merge(crate::chat_actions::router()).merge(crate::spend::routes::router()))
        .merge(crate::trash::router().merge(crate::admin_app::router(k.clone())))
        .layer(axum::middleware::from_fn_with_state(k.clone(), crate::admin_app::settings_gate::gate)) // an admin change outside Admin: step-up and audit
        .layer(axum::middleware::from_fn(cookie_guard))
        .layer(axum::middleware::from_fn(crate::update::app_version))
        .layer(axum::middleware::from_fn_with_state(k.clone(), config_tokens_on_this_machine))
        .layer(Extension(transport))
        .layer(axum::middleware::from_fn_with_state(k.clone(), require_device_cert))
        .with_state(k)
}

/// A tunnel request, once device certificates are configured, must present
/// a paired device. The header is only believed from loopback with
/// `Cf-Connecting-Ip`: that is cloudflared. A client on the LAN can set
/// the same header and is not held to it.
async fn require_device_cert(State(k): State<Arc<Kernel>>, req: Request, next: Next) -> Response {
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
    let tunnelish = req.headers().contains_key("cf-connecting-ip");
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let live = crate::devices::presented(req.headers()).is_some_and(|fp| k.devices.is_live(fp));
    let on = k.cfg.client_certs.is_some();
    let decision = match peer {
        Some(p) => crate::devices::admit(on, p, req.headers(), &method, &path, live),
        None if on && tunnelish => Err("this device is not paired with the server"),
        None => Ok(()),
    };
    if let Err(msg) = decision {
        return err(StatusCode::UNAUTHORIZED, msg);
    }
    next.run(req).await
}

/// The config's long-lived tokens (the owner's scripts, the evaluation
/// runner) work only on the plain listener, from this machine: nowhere a
/// stolen copy could be used from the network (owner's ruling 13).
async fn config_tokens_on_this_machine(State(k): State<Arc<Kernel>>, req: Request, next: Next) -> Response {
    if credential(req.headers()).is_some_and(|t| k.cfg.person_by_token(t).is_some()) {
        let plain = req.extensions().get::<Transport>() == Some(&Transport::Plain);
        let local = req.extensions().get::<axum::extract::ConnectInfo<SocketAddr>>().is_some_and(|c| c.0.ip().is_loopback());
        if !(plain && local) {
            return err(StatusCode::UNAUTHORIZED, "This key works only on the server itself, over its local address. Sign in to use Magnus from elsewhere.");
        }
    }
    next.run(req).await
}

/// Sign-in hands out a session, so over plain HTTP only from this machine:
/// neither a password nor a session crosses the network unencrypted.
pub(crate) fn over_tls(k: &Kernel, t: Transport, peer: SocketAddr) -> Result<(), Response> {
    if t == Transport::Plain && !peer.ip().is_loopback() {
        let hint = match &k.cfg.appliance.tls_listen {
            Some(l) => format!("sign in over https (port {})", l.rsplit(':').next().unwrap_or_default()),
            None => "sign in over https; the owner has not turned it on (appliance.tls_listen)".into(),
        };
        return Err(err(StatusCode::FORBIDDEN, hint));
    }
    Ok(())
}

/// Open a session for a person and hand it over: to the app as a token,
/// to a browser as an HttpOnly cookie the page cannot read.
pub(crate) async fn issue(k: &Arc<Kernel>, t: Transport, person: &str, app: bool, device: Option<&str>) -> Response {
    if let Err(e) = k.accounts.refresh_people().await {
        return internal(e);
    }
    let Some(p) = k.person(person) else { return err(StatusCode::FORBIDDEN, format!("{person} is not a person on this appliance.")) };
    let client = if app { "app" } else { "browser" };
    let (session, token) = match k.accounts.open_session(person, client, device).await {
        Ok(s) => s,
        Err(e) => return internal(e),
    };
    let ev = Ev::SignedIn { person: person.to_string(), session: session.id.clone(), client: client.into() };
    if let Err(e) = k.emit(&format!("person:{person}"), ev).await {
        return internal(e);
    }
    // Their cold start: Magnus opens a conversation, in the background.
    k.open_after_sign_in(person);
    let mut body = json!({
        "person": { "id": p.id, "name": p.name, "roles": p.roles, "groups": p.groups },
        "session": { "id": session.id, "expires": session.expires },
    });
    if app {
        body["token"] = json!(token);
        return Json(body).into_response();
    }
    let secure = if t == Transport::Tls { "; Secure" } else { "" };
    let max_age = (session.expires - session.created).num_seconds();
    let mut r = Json(body).into_response();
    if let Ok(v) = format!("{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}").parse() {
        r.headers_mut().insert("set-cookie", v);
    }
    r
}

/// How people sign in here: through the identity provider when one is
/// configured, with a password always open as the way in when it is down.
async fn sign_in_method(State(k): K) -> Response {
    let mut body = match k.oidc.get() {
        Some(o) => json!({ "method": "oidc", "label": o.label(), "password": true }),
        None => json!({ "method": "password", "password": true }),
    };
    if let Some(c) = &k.cfg.client_certs {
        let host = c.hostname.trim().trim_start_matches("https://").trim_end_matches('/');
        body["public_url"] = json!(format!("https://{host}"));
        body["device_certificates"] = json!(true);
    }
    Json(body).into_response()
}

/// The paired device a tunnel request presented, if its certificate is live.
fn device_of(k: &Kernel, peer: SocketAddr, h: &HeaderMap) -> Option<String> {
    if !crate::devices::is_tunnel(peer, h) {
        return None;
    }
    crate::devices::presented(h).and_then(|fp| k.devices.live_id(fp))
}

fn device_error(e: crate::devices::IssueError) -> Response {
    use crate::devices::IssueError;
    match e {
        IssueError::Rejected(m) => err(StatusCode::BAD_REQUEST, m),
        IssueError::Limited(m) => err(StatusCode::TOO_MANY_REQUESTS, m),
        IssueError::Unavailable(m) => err(StatusCode::SERVICE_UNAVAILABLE, m),
        IssueError::Upstream(m) => err(StatusCode::BAD_GATEWAY, m),
    }
}

fn cloudflare_of(k: &Kernel) -> Result<crate::devices::Cloudflare, Response> {
    let Some(cfg) = k.cfg.client_certs.clone() else { return Err(err(StatusCode::NOT_FOUND, "this server does not issue device certificates")) };
    if k.vault.is_sealed() {
        return Err(err(StatusCode::CONFLICT, "The kill switch is on, so a device certificate cannot be issued or revoked at Cloudflare until the owner releases it."));
    }
    let token = match k.vault.get(&cfg.credential) {
        Ok(Some(s)) => s.expose().to_string(),
        Ok(None) => return Err(err(StatusCode::SERVICE_UNAVAILABLE, format!("The vault has no secret {} for device certificates.", cfg.credential))),
        Err(e) => return Err(internal(e)),
    };
    Ok(crate::devices::Cloudflare { api: crate::devices::CF_API.into(), zone: cfg.zone_id, token, days: cfg.days })
}

/// Pair this computer. The body is the certificate request the computer
/// made; the private key stays there. Refused through the tunnel, so the
/// answer never crosses Cloudflare: pair on the appliance's own network,
/// then the public name accepts the certificate.
async fn enroll_device(State(k): K, ConnectInfo(peer): ConnectInfo<SocketAddr>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if k.cfg.client_certs.is_none() {
        return err(StatusCode::NOT_FOUND, "this server does not issue device certificates");
    }
    if crate::devices::is_tunnel(peer, &h) {
        return err(
            StatusCode::FORBIDDEN,
            "Pair this computer from the server's own network. The public address accepts a computer that is already paired and does not hand out a certificate.",
        );
    }
    if let Some(r) = refused_by_kill_switch(&k) {
        return r;
    }
    let person = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let cf = match cloudflare_of(&k) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let issued = match k.devices.issue(&cf, &person.id, b["name"].as_str().unwrap_or(""), b["csr"].as_str().unwrap_or("")).await {
        Ok(i) => i,
        Err(e) => return device_error(e),
    };
    if let Some(token) = credential(&h) {
        if let Err(e) = k.accounts.bind_session_device(token, &issued.device).await {
            tracing::error!("pair: the session was not bound to {}: {e:#}", issued.device);
        }
    }
    let paired_name = k.devices.get(&issued.device).map(|d| d.name).unwrap_or_default();
    let ev = Ev::DevicePaired { person: person.id.clone(), device: issued.device.clone(), name: paired_name };
    if let Err(e) = k.emit(&format!("person:{}", person.id), ev).await {
        tracing::error!("pair: the certificate was issued but not recorded: {e:#}");
    }
    let host = k.cfg.client_certs.as_ref().map(|c| c.hostname.trim().trim_start_matches("https://").trim_end_matches('/').to_string()).unwrap_or_default();
    Json(json!({
        "device": issued.device,
        "fingerprint": issued.fingerprint,
        "certificate": issued.certificate,
        "public_url": format!("https://{host}"),
    }))
    .into_response()
}

async fn list_devices(State(k): K, h: HeaderMap) -> Response {
    let person = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let mine = if person.is_owner() { None } else { Some(person.id.as_str()) };
    let devices: Vec<Value> = k
        .devices
        .list(mine)
        .into_iter()
        .map(|d| json!({ "id": d.id, "person": d.person, "name": d.name, "created": d.created, "revoked": d.revoked, "fingerprint": d.fingerprint }))
        .collect();
    Json(json!({ "devices": devices })).into_response()
}

async fn revoke_device(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let person = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(device) = k.devices.get(&id) else { return err(StatusCode::NOT_FOUND, "no such device") };
    if device.person != person.id && !person.is_owner() {
        return err(StatusCode::NOT_FOUND, "no such device");
    }
    let cf = match cloudflare_of(&k) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(e) = k.devices.revoke(&cf, &id).await {
        return device_error(e);
    }
    match k.accounts.end_device(&id).await {
        Ok(ended) => {
            for s in ended {
                if let Err(e) = k.emit(&format!("person:{}", s.person), Ev::SignedOut { person: s.person.clone(), session: s.id }).await {
                    tracing::error!("revoke: a session was ended but not recorded: {e:#}");
                }
            }
        }
        Err(e) => tracing::error!("revoke: sessions of {id} were not ended: {e:#}"),
    }
    let ev = Ev::DeviceRevoked { person: device.person.clone(), device: id, by: person.id.clone() };
    if let Err(e) = k.emit(&format!("person:{}", device.person), ev).await {
        tracing::error!("revoke: the certificate was revoked but not recorded: {e:#}");
    }
    Json(json!({ "ok": true })).into_response()
}

const SIGN_IN_BEGIN_WINDOW: std::time::Duration = std::time::Duration::from_secs(10 * 60);
/// Sign-ins begun from one address in the last ten minutes: at most a
/// quarter of the provider's waiting ones (`max_pending`), which everyone
/// shares, so no one address can fill them and stop every first sign-in.
fn may_begin(k: &Kernel, peer: SocketAddr) -> bool {
    let most = k.cfg.sign_in.oidc.as_ref().map_or(25, |o| (o.max_pending / 4).max(5));
    static BEGUN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<std::net::IpAddr, Vec<std::time::Instant>>>> = std::sync::OnceLock::new();
    let mut m = BEGUN.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()); // a panic elsewhere leaves counts that are still counts
    m.retain(|_, v| {
        v.retain(|t| t.elapsed() < SIGN_IN_BEGIN_WINDOW);
        !v.is_empty()
    });
    let v = m.entry(peer.ip().to_canonical()).or_default();
    if v.len() >= most {
        return false;
    }
    v.push(std::time::Instant::now());
    true
}

fn too_many_begins() -> Response {
    err(StatusCode::TOO_MANY_REQUESTS, "Too many sign-ins were started from here; finish one, or try again in a few minutes.")
}

/// A request made on the appliance's own machine: from a loopback address,
/// and not passed on by a proxy or tunnel there, which connects from
/// loopback for callers anywhere and says so in one of these headers.
pub(crate) fn on_this_machine(peer: SocketAddr, h: &HeaderMap) -> bool {
    peer.ip().to_canonical().is_loopback() && !["forwarded", "x-forwarded-for", "x-real-ip", "cf-connecting-ip"].iter().any(|n| h.contains_key(*n))
}

/// Username and password, typed in the app. With an identity provider the
/// provider checks them (and the code from an authenticator app, when the
/// person uses one); the owner's password set on the appliance is their
/// backup when the provider is down or refuses, on the appliance's own
/// machine only, since it asks for no second factor; nobody else's counts.
/// Without one, only the appliance's own passwords.
async fn sign_in(State(k): K, Extension(t): Extension<Transport>, ConnectInfo(peer): ConnectInfo<SocketAddr>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if let Err(r) = over_tls(&k, t, peer) {
        return r;
    }
    let person = crate::sign_in_name::resolve(&k.accounts, b["person"].as_str().unwrap_or_default()).await;
    let password = b["password"].as_str().unwrap_or_default();
    let code = b["code"].as_str().map(str::trim).filter(|c| !c.is_empty());
    let app = b["client"].as_str() == Some("app");
    if let Some(r) = app.then(|| crate::update::unversioned_app(&h)).flatten() {
        return r;
    }
    let device = device_of(&k, peer, &h);
    // Wrong passwords are counted where they come from: on the appliance's
    // own machine apart from the network, so the network cannot lock the
    // person out of the appliance itself.
    let counter = if on_this_machine(peer, &h) { crate::accounts::Accounts::local_counter(&person) } else { crate::accounts::Accounts::network_counter(&person) };
    let locked = |until: chrono::DateTime<chrono::Utc>| {
        (StatusCode::TOO_MANY_REQUESTS, Json(json!({ "error": format!("Too many wrong passwords. Try again after {}.", until.format("%H:%M UTC")), "retry_at": until })))
            .into_response()
    };
    // A person only the appliance itself keeps signs in against the local
    // store below, not the provider: Keycloak never heard of them (#429).
    // A person the config knows keeps the provider path, even with a local
    // namesake, as `resolve_person` resolves the config first.
    let mut provider = k.identity();
    if provider.is_some() && k.cfg.person(&person).is_none() && k.accounts.is_local_fresh(&person).await {
        provider = None;
    }
    if let Some(id) = provider {
        // The owner's backup: their password set on the appliance and a
        // one-time code printed on its machine (`magnus sign-in-code`),
        // which no relay or tunnel can carry. Counted apart, so wrong
        // passwords sent to the provider cannot lock the owner out of it.
        if let Some(machine) = b["machine_code"].as_str().map(str::trim).filter(|c| !c.is_empty()) {
            let counter = crate::accounts::Accounts::backup_counter(&person);
            if !(k.may_use_password(&person) && k.accounts.has_password(&person).await && on_this_machine(peer, &h)) {
                return err(StatusCode::UNAUTHORIZED, "The name, password or code is wrong.");
            }
            if let Some(until) = k.accounts.locked(&counter) {
                return locked(until);
            }
            match k.accounts.take_machine_code(machine).await {
                Ok(true) => {}
                Ok(false) => {
                    k.accounts.failed(&counter);
                    return err(StatusCode::UNAUTHORIZED, "The name, password or code is wrong.");
                }
                Err(e) => return internal(e),
            }
            return match k.accounts.sign_in_counted(&person, password, &counter).await {
                Ok(Ok(_)) => issue(&k, t, &person, app, device.as_deref()).await,
                // Unreachable: the password-only check never asks for the
                // authenticator code (the machine code is already the second
                // factor here), and a temporary password never takes the
                // backup. Answered as wrong, like everything else.
                Ok(Err(crate::accounts::SignInError::Rejected | crate::accounts::SignInError::NeedsCode | crate::accounts::SignInError::MustChoose)) => {
                    err(StatusCode::UNAUTHORIZED, "The name, password or code is wrong.")
                }
                Ok(Err(crate::accounts::SignInError::Locked(until))) => locked(until),
                Err(e) => internal(e),
            };
        }
        if let Some(until) = k.accounts.locked(&counter) {
            return locked(until);
        }
        // The first time, the provider's own page signs them in: it can walk
        // them through what a new account needs.
        if !k.read(|s| s.signed_in_before.contains(&person)) {
            return provider_answered(
                &k,
                t,
                id.label(),
                crate::identity::Polled::UseBrowser(format!("The first time, sign in on the {} page that just opened in your browser.", id.label())),
                device.clone(),
            )
            .await;
        }
        // Counted before the provider answers, so requests sent at once
        // cannot all pass the lockout first; a right answer clears it.
        let ours = k.cfg.person(&person).is_some();
        if ours {
            k.accounts.failed(&counter);
        }
        let answer = match id.password(&person, password, code, app).await {
            Ok(polled @ crate::identity::Polled::Done { .. }) => {
                k.accounts.succeeded(&counter);
                return provider_answered(&k, t, id.label(), polled, device.clone()).await;
            }
            Ok(polled) => Some(polled),
            Err(e) => {
                tracing::warn!("identity: {} could not check a password: {e:#}", id.label());
                None
            }
        };
        return match answer {
            Some(polled) => {
                // Only a refusal says the password was wrong: one that needs
                // the code or the browser was right, and stays uncounted.
                if ours && !matches!(polled, crate::identity::Polled::Refused(_)) {
                    k.accounts.forgive(&counter);
                }
                provider_answered(&k, t, id.label(), polled, device.clone()).await
            }
            None => {
                // Down: the attempt said nothing about the password.
                if ours {
                    k.accounts.forgive(&counter);
                }
                let backup = k.may_use_password(&person) && k.accounts.has_password(&person).await && on_this_machine(peer, &h);
                if backup {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({ "machine_code_needed": true, "error": format!("{} is not answering. On the appliance, run `magnus sign-in-code` and enter the code it prints, with your password.", id.label()) })),
                    )
                        .into_response();
                }
                err(StatusCode::BAD_GATEWAY, format!("{} is not answering. Try again in a few minutes.", id.label()))
            }
        };
    }
    if k.cfg.person(&person).is_none() && !k.accounts.is_local_fresh(&person).await {
        // Same answer and the same work as a wrong password.
        let _ = k.accounts.sign_in(&person, password).await;
        return err(StatusCode::UNAUTHORIZED, "The name or password is wrong.");
    }
    match k.accounts.sign_in_counted_with_code(&person, password, code, &counter).await {
        Ok(Ok(_)) => issue(&k, t, &person, app, device.as_deref()).await,
        Ok(Err(crate::accounts::SignInError::NeedsCode)) => code_needed(),
        Ok(Err(crate::accounts::SignInError::MustChoose)) => {
            (StatusCode::UNAUTHORIZED, Json(json!({ "must_choose": true, "error": "Choose your own password first." }))).into_response()
        }
        Ok(Err(crate::accounts::SignInError::Rejected)) => err(StatusCode::UNAUTHORIZED, "The name or password is wrong."),
        Ok(Err(crate::accounts::SignInError::Locked(until))) => locked(until),
        Err(e) => internal(e),
    }
}

/// Choose the permanent password with a temporary one: the console's
/// one-time password, or any temp the owner set. A wrong temp answers
/// as a wrong password; choosing activates, as accepting an invite
/// does. No session opens here — they sign in after, with the new one.
async fn choose_password(State(k): K, Extension(t): Extension<Transport>, ConnectInfo(peer): ConnectInfo<SocketAddr>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if let Err(r) = over_tls(&k, t, peer) {
        return r;
    }
    let person = crate::sign_in_name::resolve(&k.accounts, b["person"].as_str().unwrap_or_default()).await;
    let temp = b["password"].as_str().unwrap_or_default();
    let password = b["new_password"].as_str().unwrap_or_default();
    if password.chars().count() < k.cfg.sign_in.min_password_length {
        return err(StatusCode::BAD_REQUEST, format!("a password needs at least {} characters", k.cfg.sign_in.min_password_length));
    }
    let counter = if on_this_machine(peer, &h) { crate::accounts::Accounts::local_counter(&person) } else { crate::accounts::Accounts::network_counter(&person) };
    match k.accounts.choose_password(&person, temp, password, &counter).await {
        Ok(Ok(_)) => Json(json!({ "person": person })).into_response(),
        Ok(Err(crate::accounts::SignInError::Locked(until))) => {
            (StatusCode::TOO_MANY_REQUESTS, Json(json!({ "error": format!("Too many wrong passwords. Try again after {}.", until.format("%H:%M UTC")), "retry_at": until })))
                .into_response()
        }
        Ok(Err(_)) => err(StatusCode::UNAUTHORIZED, "The name or password is wrong."),
        Err(e) => internal(e),
    }
}

/// Ask for the 6-digit code from the person's authenticator app: the same
/// screen whether the identity provider or the local store asked (#429).
fn code_needed() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "code_needed": true, "error": "Enter the 6-digit code from your authenticator app." }))).into_response()
}

/// Start a sign-in through the identity provider: the person approves the
/// code on the provider's page, then polls `/v1/sign-in/device/{flow}`.
async fn device_start(State(k): K, Extension(t): Extension<Transport>, ConnectInfo(peer): ConnectInfo<SocketAddr>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if let Err(r) = over_tls(&k, t, peer) {
        return r;
    }
    if let Some(r) = (b["client"].as_str() == Some("app")).then(|| crate::update::unversioned_app(&h)).flatten() {
        return r;
    }
    let Some(o) = k.oidc.get() else { return err(StatusCode::NOT_FOUND, "this server signs in with a password") };
    if !may_begin(&k, peer) {
        return too_many_begins();
    }
    match o.start(b["client"].as_str() == Some("app")).await {
        Ok(s) => Json(json!({
            "flow": s.flow,
            "user_code": s.user_code,
            "verification_uri": s.verification_uri,
            "verification_uri_complete": s.verification_uri_complete,
            "expires_in": s.expires_in,
            "interval": s.interval,
            "label": o.label(),
        }))
        .into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

async fn device_poll(State(k): K, Extension(t): Extension<Transport>, ConnectInfo(peer): ConnectInfo<SocketAddr>, h: HeaderMap, Path(flow): Path<String>) -> Response {
    if let Err(r) = over_tls(&k, t, peer) {
        return r;
    }
    let Some(o) = k.oidc.get() else { return err(StatusCode::NOT_FOUND, "this server signs in with a password") };
    match o.poll(&flow).await {
        Ok(polled) => provider_answered(&k, t, o.label(), polled, device_of(&k, peer, &h)).await,
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

/// What the identity provider answered about a sign-in: wait, a session for
/// a person of this appliance with the rights the provider gives, or no.
async fn provider_answered(k: &Arc<Kernel>, t: Transport, label: String, polled: crate::identity::Polled, device: Option<String>) -> Response {
    use crate::identity::Polled;
    match polled {
        Polled::Pending => (StatusCode::ACCEPTED, Json(json!({ "pending": true }))).into_response(),
        Polled::Done { person, app, rights } => {
            if k.cfg.person(&person).is_none() {
                tracing::warn!("identity: {person} signed in at {label} but is not a person on this appliance");
                return err(StatusCode::FORBIDDEN, format!("You signed in as {person}, who is not a person on this appliance. Ask the owner to add you."));
            }
            // A person is the provider's account that first signed in as
            // them (its `sub`), not a username: an account made again, or
            // renamed, under the same name is someone else (owner's
            // rulings, 2026-11-01).
            if let Some(sub) = rights.subject.clone() {
                let _decide = k.decide.lock().await;
                match k.read(|s| s.subjects.get(&person).cloned()) {
                    Some(bound) if bound != sub => {
                        tracing::warn!("identity: an account at {label} other than {person}'s signed in as {person}; refused");
                        return err(
                            StatusCode::FORBIDDEN,
                            format!("This {label} account is not the one {person} signed in with before, so it cannot be {person} here. If {person}'s account was made again, ask the owner to let it sign in."),
                        );
                    }
                    Some(_) => {}
                    None => {
                        if let Err(e) = k.emit(&format!("person:{person}"), Ev::SubjectBound { person: person.clone(), subject: Some(sub) }).await {
                            return internal(e);
                        }
                    }
                }
            }
            // What the provider says the person may do now counts from here
            // on, for every decision about them (E2).
            if rights.groups.is_some() || rights.roles.is_some() {
                let ev = Ev::RightsFromProvider { person: person.clone(), groups: rights.groups, roles: rights.roles };
                if let Err(e) = k.emit(&format!("person:{person}"), ev).await {
                    return internal(e);
                }
            }
            k.computer_soon(crate::computer::Owner::Person(person.clone()));
            issue(k, t, &person, app, device.as_deref()).await
        }
        Polled::Failed(m) => err(StatusCode::GONE, m),
        Polled::NeedsCode => code_needed(),
        Polled::Refused(m) => err(StatusCode::UNAUTHORIZED, m),
        Polled::UseBrowser(m) => (StatusCode::UNAUTHORIZED, Json(json!({ "browser": true, "error": m }))).into_response(),
    }
}

/// Begin a sign-in in the person's own browser (the app's way): it comes
/// back to `redirect_uri`, a loopback address on their computer, and the app
/// finishes it at `/v1/sign-in/browser/finish`.
async fn browser_begin(State(k): K, Extension(t): Extension<Transport>, ConnectInfo(peer): ConnectInfo<SocketAddr>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if let Err(r) = over_tls(&k, t, peer) {
        return r;
    }
    if let Some(r) = (b["client"].as_str() == Some("app")).then(|| crate::update::unversioned_app(&h)).flatten() {
        return r;
    }
    let Some(id) = k.identity() else { return err(StatusCode::NOT_FOUND, "this server signs in with a password") };
    let Some(redirect_uri) = b["redirect_uri"].as_str() else { return err(StatusCode::BAD_REQUEST, "name the redirect address") };
    if !may_begin(&k, peer) {
        return too_many_begins();
    }
    if let Err(e) = crate::connectors::loopback(redirect_uri) {
        return err(StatusCode::BAD_REQUEST, format!("{e:#}"));
    }
    let hint = b["login_hint"].as_str().map(str::trim).filter(|h| !h.is_empty());
    match id.begin(redirect_uri, hint, b["client"].as_str() == Some("app")).await {
        Ok(s) => Json(json!({ "state": s.state, "authorize_url": s.authorize_url, "expires_in": s.expires_in, "label": id.label() })).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

/// Finish a browser sign-in with what came back to the loopback address.
async fn browser_finish(State(k): K, Extension(t): Extension<Transport>, ConnectInfo(peer): ConnectInfo<SocketAddr>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if let Err(r) = over_tls(&k, t, peer) {
        return r;
    }
    let Some(id) = k.identity() else { return err(StatusCode::NOT_FOUND, "this server signs in with a password") };
    let (Some(state), Some(code)) = (b["state"].as_str(), b["code"].as_str()) else { return err(StatusCode::BAD_REQUEST, "send the state and code that came back") };
    match id.finish(state, code, b["iss"].as_str()).await {
        Ok(polled) => provider_answered(&k, t, id.label(), polled, device_of(&k, peer, &h)).await,
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

/// Accept an invitation: the token with the new password activates the
/// person, who then signs in normally. A bad, used or expired token, or
/// one for someone no longer here, answers alike.
async fn accept_invite(State(k): K, Extension(t): Extension<Transport>, ConnectInfo(peer): ConnectInfo<SocketAddr>, Json(b): Json<Value>) -> Response {
    if let Err(r) = over_tls(&k, t, peer) {
        return r;
    }
    let token = b["token"].as_str().unwrap_or_default();
    let password = b["password"].as_str().unwrap_or_default();
    if password.chars().count() < k.cfg.sign_in.min_password_length {
        return err(StatusCode::BAD_REQUEST, format!("a password needs at least {} characters", k.cfg.sign_in.min_password_length));
    }
    match k.accept_invitation(token, password).await.map(|a| a.map(|a| a.person)) {
        Ok(Some(person)) if k.cfg.person(&person).is_some() || k.accounts.is_local(&person) => {
            k.computer_soon(crate::computer::Owner::Person(person.clone()));
            Json(json!({ "person": person })).into_response()
        }
        Ok(_) => err(StatusCode::UNAUTHORIZED, "The invitation is wrong or expired."),
        Err(e) => internal(e),
    }
}

fn h_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn reset_shell(title: &str, body: &str) -> Response {
    (
        StatusCode::OK,
        [("content-type", "text/html; charset=utf-8")],
        format!(
            "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>{title}</title><style>body{{font-family:system-ui,sans-serif;max-width:28em;margin:4em auto;padding:0 1em;color:#1a1a1a}}input,button{{font:inherit}}input{{display:block;width:100%;box-sizing:border-box;margin:.4em 0 1em;padding:.5em}}button{{padding:.5em 1.2em}}.error{{color:#a00}}</style></head><body>{body}</body></html>"
        ),
    )
        .into_response()
}

/// The recovery page behind an emailed link: a live token gets the form,
/// anything else an explanation and no oracle beyond it.
async fn reset_page(
    State(k): K,
    Extension(t): Extension<Transport>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Response {
    if let Err(r) = over_tls(&k, t, peer) {
        return r;
    }
    let token = q.get("token").map(|s| s.trim().to_string()).unwrap_or_default();
    if token.is_empty() || !k.accounts.reset_live(&token).await {
        return reset_shell("Link expired", "<h1>This recovery link is spent or expired.</h1><p>Ask for a new email and follow its link within the hour.</p>");
    }
    reset_shell(
        "Choose a new password",
        &format!(
            "<h1>Choose a new password</h1><p>It signs every device out, and any authenticator codes stop working until new ones are enrolled.</p><form method=post action=/reset><input type=hidden name=token value=\"{}\"><label>New password<input type=password name=password autocomplete=new-password required></label><label>Repeat it<input type=password name=confirm autocomplete=new-password required></label><button>Set password</button></form>",
            h_escape(&token)
        ),
    )
}

/// Set the password from the recovery page.
async fn reset_confirm(
    State(k): K,
    Extension(t): Extension<Transport>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Form(f): Form<std::collections::HashMap<String, String>>,
) -> Response {
    if let Err(r) = over_tls(&k, t, peer) {
        return r;
    }
    let token = f.get("token").map(|s| s.trim().to_string()).unwrap_or_default();
    let password = f.get("password").cloned().unwrap_or_default();
    if f.get("confirm").map(|s| s.as_str()) != Some(password.as_str()) {
        return reset_shell("Choose a new password", &format!("<h1>Choose a new password</h1><p class=error>The two passwords do not match.</p><form method=post action=/reset><input type=hidden name=token value=\"{}\"><label>New password<input type=password name=password autocomplete=new-password required></label><label>Repeat it<input type=password name=confirm autocomplete=new-password required></label><button>Set password</button></form>", h_escape(&token)));
    }
    if password.chars().count() < k.cfg.sign_in.min_password_length {
        return reset_shell("Choose a new password", &format!("<h1>Choose a new password</h1><p class=error>A password needs at least {} characters.</p><form method=post action=/reset><input type=hidden name=token value=\"{}\"><label>New password<input type=password name=password autocomplete=new-password required></label><label>Repeat it<input type=password name=confirm autocomplete=new-password required></label><button>Set password</button></form>", k.cfg.sign_in.min_password_length, h_escape(&token)));
    }
    match k.accounts.confirm_reset(&token, &password).await {
        Ok(Some((person, ended))) => {
            let mut evs = vec![(person.as_str(), Ev::PasswordChanged { person: person.clone(), how: "reset".into() })];
            evs.extend(ended.into_iter().map(|session| (person.as_str(), Ev::SignedOut { person: person.clone(), session })));
            if let Err(e) = k.emit_all(evs).await {
                return internal(e);
            }
            reset_shell(
                "Password set",
                &format!("<h1>The new password is set.</h1><p>Sign in as <b>{}</b> or with your email address. Every device is signed out. If codes were enrolled, they no longer work: enroll new ones after signing in.</p>", h_escape(&person)),
            )
        }
        Ok(None) => reset_shell("Link expired", "<h1>This recovery link is spent or expired.</h1><p>Ask for a new email and follow its link within the hour.</p>"),
        Err(e) => internal(e),
    }
}

async fn sign_out(State(k): K, h: HeaderMap) -> Response {
    let Some(token) = credential(&h) else { return err(StatusCode::UNAUTHORIZED, "not signed in") };
    match k.accounts.sign_out(token).await {
        Ok(Some(s)) => {
            if let Err(e) = k.emit(&format!("person:{}", s.person), Ev::SignedOut { person: s.person.clone(), session: s.id }).await {
                return internal(e);
            }
        }
        Ok(None) => {}
        Err(e) => return internal(e),
    }
    let mut r = Json(json!({ "ok": true })).into_response();
    if let Ok(v) = format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0").parse() {
        r.headers_mut().insert("set-cookie", v);
    }
    r
}

async fn me(State(k): K, h: HeaderMap) -> Response {
    match who(&k, &h).await {
        Ok(p) => Json(json!({ "id": p.id, "name": p.name, "groups": p.groups, "roles": p.roles, "voice": k.cfg.speech.is_some(), "read_aloud": k.cfg.read_aloud.is_some() }))
            .into_response(),
        Err(r) => r,
    }
}

/// What the appliance is running now, and every change to it, so apps can
/// tell people when the agent they work with has changed (U5).
async fn versions(State(k): K, h: HeaderMap) -> Response {
    if let Err(r) = who(&k, &h).await {
        return r;
    }
    let v = k.read(|s| {
        let changes: Vec<Value> = s.version_changes.iter().map(|(seq, at, what, value)| json!({ "seq": seq, "at": at, "what": what, "value": value })).collect();
        json!({
            "harness": crate::kernel::HARNESS_VERSION,
            "instructions": crate::run::instructions_version(),
            "chat_instructions": crate::run::chat_instructions_version(),
            "model": s.model.as_ref().map(|(a, m)| format!("{a}/{m}")),
            "playbook": s.playbook_version,
            "latest_seq": s.version_changes.last().map(|c| c.0),
            "changes": changes,
        })
    });
    Json(v).into_response()
}

pub(crate) fn status_str(s: Status) -> &'static str {
    match s {
        Status::Running => "running",
        Status::Suspended => "suspended",
        Status::Completed => "completed",
        Status::Failed => "failed",
        Status::Cancelled => "cancelled",
    }
}

/// The short title the model gave the task (when starting it, in its plan,
/// or in a contract recorded before rev 147), else the start of what the person wrote; from its words,
/// recalled for its person.
pub(crate) fn title_of(task: &crate::state::Task, run: Option<&Run>, words: &crate::recall::Words) -> String {
    words
        .title(task.seq)
        .or_else(|| run.and_then(|r| r.plan.as_ref().or(r.contract.as_ref())).and_then(|c| c["title"].as_str()).map(str::trim).filter(|t| !t.is_empty()).map(String::from))
        .unwrap_or_else(|| words.text(task.seq).chars().take(80).collect())
}

/// Whether the task waits on an inbox item this person can answer.
fn needs(s: &crate::state::State, p: &Person, run: &Run) -> bool {
    run.status == Status::Suspended && s.items.values().any(|i| i.run.as_deref() == Some(run.id.as_str()) && i.resolution.is_none() && inbox::may_respond(s, p, i))
}

/// A task in a list: what it is, where it stands, whether it needs this
/// person, and the request it came from (a chat message or a schedule).
/// `words` holds the recalled words of the tasks and schedules it names.
pub(crate) fn task_row(k: &Kernel, s: &crate::state::State, p: &Person, t: &crate::state::Task, r: &Run, words: &crate::recall::Words) -> Value {
    let mut v = task_json(k, t, r, false, words);
    let mine = t.person == p.id;
    v["needs_you"] = json!(needs(s, p, r));
    // Another person's projects and conversations are theirs alone: the
    // owner sees that a task came from a chat, not what was said in it.
    v["project"] = if mine { crate::library::project_ref(s, &t.task_id) } else { Value::Null };
    v["waiting_item"] = json!(r.waiting_item);
    v["ended_at"] = json!(r.ended_at);
    // A scheduled run's sub-agents are grouped under its schedule.
    v["group"] = if let Some(sc) = s.authority(&t.run_id).and_then(|a| a.schedule.as_ref()).and_then(|id| s.schedules.get(id)) {
        json!({ "kind": "schedule", "id": sc.id, "title": words.title(sc.seq).unwrap_or_else(|| words.text(sc.seq).to_string()) })
    } else if let Some((c, turn)) =
        t.chat.as_ref().and_then(|c| c.from_run.as_ref().and_then(|f| s.runs.get(f)).and_then(|r| s.tasks.get(&r.task)).map(|turn| (c, crate::outcomes::person_turn(s, turn))))
    {
        if mine {
            json!({ "kind": "chat", "id": turn.task_id, "title": words.text(turn.seq), "thread": c.thread })
        } else {
            json!({ "kind": "chat", "id": turn.task_id, "title": format!("A conversation of {}", t.person) })
        }
    } else {
        Value::Null
    };
    v
}

fn task_json(k: &Kernel, task: &crate::state::Task, run: &Run, full: bool, words: &crate::recall::Words) -> Value {
    let mut v = json!({
        "task_id": task.task_id,
        "run_id": run.id,
        "person": task.person,
        "text": words.text(task.seq),
        "title": title_of(task, Some(run), words),
        "created_at": task.created_at,
        "status": status_str(run.status),
        "waiting_on": run.reason,
        "status_line": crate::evidence::status_line(run),
        "summary": run.summary,
        "thread": task.chat.as_ref().map(|c| c.thread.clone()),
        "verdict": task.verdict,
        "review": task.review,
        "poll": task.poll,
    });
    // The apps this task asked the person to connect, each as they stand
    // with it now (connectors spec, Inside work).
    if full {
        let mut offered: Vec<(i64, Value)> =
            run.results.values().filter_map(|r| r.data.as_ref().filter(|d| d["connector"].is_string()).map(|d| (r.seq, k.offered_card(&task.person, d)))).collect();
        offered.sort_by_key(|(seq, _)| *seq);
        v["app_offers"] = json!(offered.into_iter().map(|(_, c)| c).collect::<Vec<_>>());
    }
    if full {
        // Evidence from the record (spec rev 147): receipts with their
        // read-backs, what was not done, the sources the result cites.
        v["evidence"] = k.evidence_of(run);
        // A task from before rev 147 keeps the contract it recorded, as data.
        v["contract"] = run.contract.clone().unwrap_or(Value::Null);
        v["model"] = json!({ "adapter": run.config.adapter, "name": run.config.model });
        v["usage"] = json!({ "model_calls": run.model_calls, "actions": run.actions, "cost_usd": run.usd });
        // What the run may spend: for a task from a proposal, the estimate the person approved (#254).
        v["budget"] = json!(run.config.budget);
        v["taint"] = json!({ "untrusted": run.untrusted, "private": run.private });
        v["outputs"] = json!(run.outputs.iter().map(|(id, name, mt, size)| json!({ "id": id, "name": name, "media_type": mt, "size": size })).collect::<Vec<_>>());
        v["plan"] = run.plan.clone().unwrap_or(Value::Null);
        v["plan_version"] = json!(run.plan_version);
        v["outcome_view"] = run.outcome_view.clone().unwrap_or(Value::Null);
        // Only here, never in a list's rows: those are built under the state lock.
        v["files"] = json!(k.read(|s| task.files.iter().filter_map(|id| s.files.get(id)).map(|f| crate::library::file_json(s, f, words)).collect::<Vec<_>>()));
        v["project"] = k.read(|s| crate::library::project_ref(s, &task.task_id));
        v["preview"] = json!({ "chars": crate::compose::PREVIEW_CHARS, "rows": crate::compose::MAX_ROWS });
        v["spoken"] = json!(run.spoken);
        v["ended_at"] = json!(run.ended_at);
        v["waiting_item"] = json!(run.waiting_item);
        v["config"] = json!({ "harness": run.config.harness, "instructions": run.config.instructions, "playbook": run.config.playbook });
        v["open_items"] = json!(k.read(|s| s.items.values().filter(|i| i.run.as_deref() == Some(&run.id) && i.resolution.is_none()).map(|i| i.id.clone()).collect::<Vec<_>>()));
    }
    v
}

async fn submit(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let text = b["text"].as_str().unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return err(StatusCode::BAD_REQUEST, "the task has no text");
    }
    let mut files = match crate::library::given_files(&k, &p, &b) {
        Ok(f) => f,
        Err(r) => return r,
    };
    let project = match crate::library::chosen_project(&k, &p, &b) {
        Ok(x) => x,
        Err(r) => return r,
    };
    let model = match chosen_model(&k, &b).await {
        Ok(m) => m,
        Err(r) => return r,
    };
    // Files sent with the task are uploads like any other: they go to the
    // person's library, linked to this task.
    for a in b["attachments"].as_array().into_iter().flatten() {
        match crate::library::add_file(&k, &p, a, None).await {
            Ok(Ok(id)) => files.push(id),
            Ok(Err(r)) => return r,
            Err(e) => return internal(e),
        }
    }
    let o = crate::kernel::NewRun { model, files, project, ..Default::default() };
    match k.create_run(&format!("person:{}", p.id), &p.id, text, crate::attachment::from_request(&b), o).await {
        Ok((task, run)) => (StatusCode::CREATED, Json(json!({ "task_id": task, "run_id": run }))).into_response(),
        Err(e) => internal(e),
    }
}

/// The model a request chose (`"model": {"adapter", "model"}`), if any,
/// once its adapter confirms it offers it.
pub(crate) async fn chosen_model(k: &Kernel, b: &Value) -> Result<Option<(String, String)>, Response> {
    if b["model"].is_null() {
        return Ok(None);
    }
    let (Some(adapter), Some(model)) = (b["model"]["adapter"].as_str(), b["model"]["model"].as_str()) else {
        return Err(err(StatusCode::BAD_REQUEST, "model needs adapter and model"));
    };
    match k.check_model(adapter, model).await {
        Ok(()) => Ok(Some((adapter.into(), model.into()))),
        Err(m) => Err(err(StatusCode::BAD_REQUEST, m)),
    }
}

/// Each person's tasks are their own, the owner's too (owner, 2026-10-30:
/// "Individual users should have their own accounts with their own tasks,
/// conversations, schedules, memory").
fn visible_task(p: &Person, t: &crate::state::Task) -> bool {
    t.person == p.id
}

async fn list_tasks(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let words = match k.person_words(&p.id).await {
        Ok(w) => w,
        Err(e) => return internal(e),
    };
    let tasks: Vec<Value> = k.read(|s| {
        s.tasks
            .values()
            // Polls fire dozens a day: they stay out of the person's
            // lists, auditable by id and in the log (#406).
            .filter(|t| visible_task(&p, t) && !s.in_chat(t) && !t.poll)
            .filter_map(|t| s.runs.get(&t.run_id).map(|r| task_row(&k, s, &p, t, r, &words)))
            .collect()
    });
    Json(json!({ "tasks": tasks })).into_response()
}

/// A search's words, lowercased; a match has every one.
pub(crate) fn terms(q: &str) -> Vec<String> {
    q.to_lowercase().split_whitespace().map(String::from).collect()
}

/// One search across the person's conversations (their titles and what was
/// said in them), tasks (title, request, summary) and files (as the library
/// matches them). A conversation comes with the first thing said in it that
/// matches.
async fn search(State(k): K, h: HeaderMap, Query(q): Query<std::collections::HashMap<String, String>>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let terms = terms(q.get("q").map(String::as_str).unwrap_or_default());
    if terms.is_empty() {
        return Json(json!({ "conversations": [], "tasks": [], "files": [] })).into_response();
    }
    let all = |hay: &str| terms.iter().all(|t| hay.contains(t.as_str()));
    let mut conversations = vec![];
    let threads = match k.threads(&p.id).await {
        Ok(t) => t,
        Err(e) => return internal(e),
    };
    for t in threads {
        let id = t["thread"].as_str().unwrap_or_default();
        let said: Vec<String> = match k.thread_entries(&p.id, id).await {
            Ok(e) => e.unwrap_or_default(),
            Err(e) => return internal(e),
        }
        .iter()
        .filter_map(|e| e["text"].as_str().or(e["summary"].as_str()).map(String::from))
        .collect();
        let title = t["title"].as_str().unwrap_or_default().to_lowercase();
        if !all(&format!("{title} {}", said.join(" ").to_lowercase())) {
            continue;
        }
        let snippet = said.iter().find(|s| terms.iter().any(|w| s.to_lowercase().contains(w.as_str()))).map(|s| s.chars().take(160).collect::<String>());
        conversations.push(json!({ "thread": id, "title": t["title"], "updated_at": t["updated_at"], "project": t["project"], "snippet": snippet }));
    }
    let texts = match k.file_texts(&p.id, &terms).await {
        Ok(t) => t,
        Err(e) => return internal(e),
    };
    let words = match k.person_words(&p.id).await {
        Ok(w) => w,
        Err(e) => return internal(e),
    };
    let (tasks, files) = k.read(|s| {
        let tasks: Vec<Value> = s
            .tasks
            .values()
            .filter(|t| visible_task(&p, t) && !s.in_chat(t) && !t.poll)
            .filter_map(|t| s.runs.get(&t.run_id).map(|r| task_row(&k, s, &p, t, r, &words)))
            .filter(|v| {
                all(&format!("{} {} {}", v["title"].as_str().unwrap_or_default(), v["text"].as_str().unwrap_or_default(), v["summary"].as_str().unwrap_or_default()).to_lowercase())
            })
            .collect();
        (tasks, crate::library::files_matching(s, &p, &terms, &texts, None, &words))
    });
    Json(json!({ "conversations": conversations, "tasks": tasks, "files": files })).into_response()
}

/// The person writes in chat: a new thread, or one of theirs.
async fn chat_post(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let text = b["text"].as_str().unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return err(StatusCode::BAD_REQUEST, "the message has no text");
    }
    let model = match chosen_model(&k, &b).await {
        Ok(m) => m,
        Err(r) => return r,
    };
    let files = match crate::library::given_files(&k, &p, &b) {
        Ok(f) => f,
        Err(r) => return r,
    };
    let project = match crate::library::chosen_project(&k, &p, &b) {
        Ok(x) => x,
        Err(r) => return r,
    };
    match k.chat_say(&p.id, text.clone(), b["thread"].as_str().map(String::from), model, None, files, project).await {
        Ok(Some((thread, task))) => (StatusCode::CREATED, Json(json!({ "thread": thread, "task_id": task, "hidden": crate::typed_secrets::mask(&text).hidden }))).into_response(),
        Ok(None) => err(StatusCode::NOT_FOUND, "no such thread"),
        Err(e) => internal(e),
    }
}

async fn chat_threads(State(k): K, h: HeaderMap) -> Response {
    match who(&k, &h).await {
        Ok(p) => match k.threads(&p.id).await {
            Ok(t) => Json(json!({ "threads": t })).into_response(),
            Err(e) => internal(e),
        },
        Err(r) => r,
    }
}

/// A thread is its starter's alone.
async fn chat_thread(State(k): K, h: HeaderMap, Path(thread): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match k.thread_entries(&p.id, &thread).await {
        // Their reactions shown on Magnus's messages: to them alone, not in
        // the thread as anyone else may read it.
        Ok(Some(entries)) => Json(json!({ "thread": thread, "entries": k.read(|s| crate::reactions::marked(s, &p.id, &thread, entries)) })).into_response(),
        Ok(None) => err(StatusCode::NOT_FOUND, "no such thread"),
        Err(e) => internal(e),
    }
}

/// Confirm or decline a schedule card; only the person it was proposed to.
async fn chat_card(State(k): K, h: HeaderMap, Path(card): Path<String>, Json(b): Json<Value>) -> Response {
    use crate::chat::CardRefusal;
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match k.decide_card(&p.id, &card, b["decision"].as_str().unwrap_or_default(), b["reason"].as_str(), &b).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(CardRefusal::NotFound)) => err(StatusCode::NOT_FOUND, "no such card"),
        Ok(Err(CardRefusal::Decided)) => err(StatusCode::CONFLICT, "this card has been decided"),
        Ok(Err(CardRefusal::Invalid(m))) => err(StatusCode::BAD_REQUEST, &m),
        Err(e) => internal(e),
    }
}

fn find(k: &Kernel, p: &Person, id: &str) -> Option<(crate::state::Task, Run)> {
    k.read(|s| {
        let t = s.tasks.get(id)?;
        if !visible_task(p, t) {
            return None;
        }
        Some((t.clone(), s.runs.get(&t.run_id)?.clone()))
    })
}

async fn task(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match find(&k, &p, &id) {
        Some((t, r)) => {
            let words = match k.person_words(&t.person).await {
                Ok(w) => w,
                Err(e) => return internal(e),
            };
            // `find` gives only the person's own.
            Json(task_json(&k, &t, &r, true, &words)).into_response()
        }
        None => err(StatusCode::NOT_FOUND, "no such task"),
    }
}

/// The task view's evidence: each step's tool calls, boundary decisions and
/// results, as derived from the log (K45).
async fn timeline(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some((_, run)) = find(&k, &p, &id) else { return err(StatusCode::NOT_FOUND, "no such task") };
    Json(timeline_of(&run, false)).into_response()
}

/// A run's steps, each with its tool calls, the boundary's decisions (their
/// stage and outcome: rules and reasons are the developer view's) and
/// their results; its notices and verifications. With `redact_private`, the
/// calls that are the person's alone keep their name but lose their words.
pub(crate) fn timeline_of(run: &Run, redact_private: bool) -> Value {
    let mut steps = vec![];
    for reply in &run.replies {
        let mut calls = vec![];
        let mut said = String::new();
        for b in &reply.blocks {
            match b {
                magnus_adapter_proto::Block::Text { text } => said.push_str(text),
                magnus_adapter_proto::Block::ToolCall { id, name, input } => {
                    let r = run.results.get(id);
                    let decisions: Vec<Value> = run.decisions.iter().filter(|d| &d.call_id == id).map(|d| json!({ "stage": d.stage, "outcome": d.outcome })).collect();
                    let hidden = redact_private && crate::tools::person_only_tool(name);
                    calls.push(json!({
                        "call_id": id, "tool": name, "input": if hidden { json!(crate::tools::REDACTED_CALL) } else { input.clone() },
                        "issuer": run.intents.get(id).map(|i| i.issuer.clone()).unwrap_or_else(|| "model".into()),
                        "decisions": decisions,
                        "result": r.map(|r| if hidden { crate::tools::REDACTED_CALL.to_string() } else { r.text.chars().take(4000).collect::<String>() }),
                        "is_error": r.map(|r| r.is_error), "status": r.map(|r| r.status.clone()),
                    }));
                }
                _ => {}
            }
        }
        steps.push(json!({ "step": reply.step, "text": said, "calls": calls }));
    }
    let notices: Vec<Value> = run.notices.iter().map(|(step, seq, text)| json!({ "step": step, "seq": seq, "text": text })).collect();
    json!({ "steps": steps, "notices": notices, "verifications": run.verified.iter().map(|(c, v)| json!({ "criterion": c, "passed": v.0, "evidence": v.1, "by": v.2 })).collect::<Vec<_>>() })
}

async fn resume(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some((_, run)) = find(&k, &p, &id) else { return err(StatusCode::NOT_FOUND, "no such task") };
    if run.status != Status::Suspended {
        return err(StatusCode::CONFLICT, "the task is not paused");
    }
    if let Some((_, why)) = k.halt_of(&run.id, None) {
        return err(StatusCode::CONFLICT, why.replace("kill switch engaged", "the kill switch is engaged"));
    }
    if let Err(e) = k.emit(&format!("person:{}", p.id), Ev::Resumed { run: run.id.clone() }).await {
        return internal(e);
    }
    k.wake(&run.id);
    Json(json!({ "ok": true })).into_response()
}

async fn cancel(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    controlled(k.cancel_task(&p, &id, None).await.map(|v| {
        let mut v = v;
        v["ok"] = json!(true);
        v
    }))
}

/// A control's answer as the app gets it.
fn controlled(r: Result<Value, crate::controls::Refused>) -> Response {
    use crate::controls::Refused;
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e @ Refused::NotFound(_)) => err(StatusCode::NOT_FOUND, e.to_string()),
        Err(Refused::Denied(m)) => err(StatusCode::FORBIDDEN, m),
        Err(Refused::Conflict(m)) => err(StatusCode::CONFLICT, m),
        Err(Refused::Invalid(m)) => err(StatusCode::BAD_REQUEST, m),
        Err(Refused::Failed(m)) => err(StatusCode::INTERNAL_SERVER_ERROR, m),
    }
}

/// The person's verdict on an outcome that has ended: right, partly or
/// wrong, with an optional note. A later verdict replaces it.
async fn verdict(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    // An experiment's trials run as the eval people, who have no sign-in of
    // their own: the owner judges them, though they see no one else's tasks.
    let trial = || k.read(|s| s.tasks.get(&id).and_then(|t| s.runs.get(&t.run_id).filter(|r| r.experiment.is_some()).map(|r| (t.clone(), r.clone()))));
    let Some((task, run)) = find(&k, &p, &id).or_else(|| p.is_owner().then(trial).flatten()) else { return err(StatusCode::NOT_FOUND, "no such task") };
    if task.person != p.id && !(p.is_owner() && run.experiment.is_some()) {
        return err(StatusCode::FORBIDDEN, "only the person it is for gives a verdict");
    }
    let Some(v) = b["verdict"].as_str().filter(|v| crate::state::VERDICTS.contains(v)) else {
        return err(StatusCode::BAD_REQUEST, "verdict is right, partly or wrong");
    };
    let note = b["note"].as_str().map(str::trim).filter(|n| !n.is_empty()).map(String::from);
    if note.as_ref().is_some_and(|n| n.chars().count() > crate::compose::MAX_TEXT) {
        return err(StatusCode::BAD_REQUEST, format!("the note is longer than {} characters", crate::compose::MAX_TEXT));
    }
    if !run.status.terminal() {
        return err(StatusCode::CONFLICT, "the task has not ended");
    }
    let ev = Ev::Verdict { task: task.task_id.clone(), person: p.id.clone(), verdict: v.into(), note };
    match k.emit(&format!("person:{}", p.id), ev).await {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => internal(e),
    }
}

/// Opening the app's home: what changed since the last open — ended
/// tasks, decided cards and new arrivals — with the queued
/// observations, newest first; the open itself is recorded so the next
/// one starts here (#367). A first open looks back a day.
async fn home_opened(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let words = match k.person_words(&p.id).await {
        Ok(w) => w,
        Err(e) => return internal(e),
    };
    let now = chrono::Utc::now();
    let (since, mut ended, mut arrived, mut decided, mut observations) = k.read(|s| {
        let since = s.home_last_open.get(&p.id).cloned().unwrap_or(now - chrono::Duration::hours(24));
        let mut ended: Vec<(chrono::DateTime<chrono::Utc>, Value)> = vec![];
        let mut arrived: Vec<(chrono::DateTime<chrono::Utc>, Value)> = vec![];
        let mut decided: Vec<(chrono::DateTime<chrono::Utc>, Value)> = vec![];
        let mut observations: Vec<(chrono::DateTime<chrono::Utc>, Value)> = vec![];
        for t in s.tasks.values().filter(|t| visible_task(&p, t) && !s.in_chat(t) && !t.review && !t.poll) {
            if let Some(r) = s.runs.get(&t.run_id) {
                if matches!(r.status, Status::Completed | Status::Failed) && r.ended_at.is_some_and(|e| e > since) {
                    ended.push((r.ended_at.unwrap(), task_row(&k, s, &p, t, r, &words)));
                }
            }
        }
        for i in s.items.values().filter(|i| inbox::visible(s, &p, i)) {
            let row = inbox::render(s, &p, i);
            if i.created_at > since {
                arrived.push((i.created_at, row.clone()));
            }
            if i.resolution.is_some() && s.items_resolved_at.get(&i.id).is_some_and(|a| *a > since) {
                decided.push((s.items_resolved_at[&i.id], row.clone()));
            }
            if i.resolution.is_none() && (i.kind == "observation" || i.kind == "interrupt") {
                observations.push((i.created_at, row));
            }
        }
        (since, ended, arrived, decided, observations)
    });
    for feed in [&mut ended, &mut arrived, &mut decided, &mut observations] {
        feed.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    }
    let feed = Json(json!({
        "since": since,
        "ended": ended.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        "arrived": arrived.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        "decided": decided.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        "observations": observations.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
    }));
    match k.emit(&format!("person:{}", p.id), Ev::HomeOpened { person: p.id.clone() }).await {
        // The feed was built before the open landed: if the open did
        // not land, the next open shows it all again.
        Ok(_) => feed.into_response(),
        Err(e) => internal(e),
    }
}

/// The app presented inbox items to its person: record each visible
/// one opened, first presentation standing (#367). Unknown ids, and
/// another person's, are ignored, so a stale app never fails the open.
async fn inbox_opened(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let (marked, fresh): (Vec<String>, Vec<String>) = k.read(|s| {
        let mut marked = vec![];
        let mut fresh = vec![];
        for id in b["ids"].as_array().cloned().unwrap_or_default().into_iter().filter_map(|v| v.as_str().map(String::from)) {
            if s.items.get(&id).is_some_and(|i| inbox::visible(s, &p, i)) {
                marked.push(id.clone());
                if !s.items_opened.contains_key(&id) {
                    fresh.push(id);
                }
            }
        }
        (marked, fresh)
    });
    if !fresh.is_empty() {
        let actor = format!("person:{}", p.id);
        let evs: Vec<(&str, Ev)> = fresh.iter().map(|id| (actor.as_str(), Ev::ItemOpened { id: id.clone() })).collect();
        if let Err(e) = k.emit_all(evs).await {
            return internal(e);
        }
    }
    Json(json!({ "opened": marked })).into_response()
}

async fn inbox_list(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let mut items: Vec<Value> = k.read(|s| s.items.values().filter(|i| inbox::visible(s, &p, i)).map(|i| inbox::render(s, &p, i)).collect());
    // An approval for an app's action carries the app's card, as the person
    // whose task it is stands with it (connectors spec: the same card in the
    // Library, in chat, in a task and in approvals).
    let apps: Vec<String> = k.connectors().into_iter().map(|c| c.name).collect();
    for i in items.iter_mut().filter(|i| i["kind"] == "approval") {
        let app = i["details"]["tool"].as_str().and_then(|t| t.split_once("__")).map(|(a, _)| a.to_string());
        if let Some(app) = app.filter(|a| apps.contains(a)) {
            i["app"] = k.offered_card(i["person"].as_str().unwrap_or_default(), &json!({ "connector": app }));
        }
    }
    Json(json!({ "items": items })).into_response()
}

async fn respond(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match k.respond(&p, &id, &b).await {
        Ok(Answer::Ok) => Json(json!({ "ok": true })).into_response(),
        Ok(Answer::Forbidden(m)) => err(StatusCode::FORBIDDEN, m),
        Ok(Answer::NotFound) => err(StatusCode::NOT_FOUND, "no such item"),
        Ok(Answer::Conflict(m)) => err(StatusCode::CONFLICT, m),
        Ok(Answer::Invalid(m)) => err(StatusCode::BAD_REQUEST, m),
        Err(e) => internal(e),
    }
}

async fn policies(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let all = k.read(|s| s.policies.clone());
    // The layers of their own tasks, which they may set (`put_policy`).
    let own_task = |l: &str| l.strip_prefix("task/").is_some_and(|t| k.read(|s| s.tasks.get(t).is_some_and(|t| t.person == p.id)));
    let mine: serde_json::Map<String, Value> = all
        .into_iter()
        .filter(|(l, _)| inbox::may_set_policy(&p, l) || l == "org" || l == &format!("user/{}", p.id) || p.groups.iter().any(|g| l == &format!("group/{g}")) || own_task(l))
        .map(|(l, c)| (l, Value::String(c)))
        .collect();
    Json(json!({ "layers": mine, "default_org": crate::policy::DEFAULT_ORG })).into_response()
}

async fn put_policy(State(k): K, h: HeaderMap, Path(layer): Path<String>, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let layer = layer.trim_matches('/').to_string();
    // A task's layer: the task's own, which only narrows, so its person
    // may set it as well as the owner (DECISIONS, Layering).
    let task_of = layer.strip_prefix("task/").and_then(|t| k.read(|s| s.tasks.get(t).map(|t| t.person.clone())));
    let valid = layer == "org"
        || layer.strip_prefix("user/").is_some_and(|u| k.cfg.person(u).is_some() || k.accounts.is_local(u))
        || layer.strip_prefix("group/").is_some_and(|g| !g.is_empty() && !g.contains('/'))
        || task_of.is_some();
    if !valid {
        return err(StatusCode::NOT_FOUND, "no such policy layer");
    }
    if !(inbox::may_set_policy(&p, &layer) || task_of.as_deref() == Some(p.id.as_str())) {
        return err(StatusCode::FORBIDDEN, "you may not set this layer's policy");
    }
    let cedar = b["cedar"].as_str().unwrap_or_default().to_string();
    if let Err(e) = crate::policy::validate(&cedar) {
        return err(StatusCode::BAD_REQUEST, format!("the policy does not parse: {e}"));
    }
    if let Err(why) = crate::policy_layers::tightens_only(&layer, &cedar) {
        return err(StatusCode::UNPROCESSABLE_ENTITY, why);
    }
    match k.emit(&format!("person:{}", p.id), Ev::PolicySet { layer, cedar }).await {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => internal(e),
    }
}

async fn secret_names(State(k): K, h: HeaderMap) -> Response {
    if let Err(r) = owner(&k, &h).await {
        return r;
    }
    match k.vault.names() {
        Ok(n) => Json(json!({ "names": crate::secret_scope::server_names(&k, n) })).into_response(),
        Err(e) => internal(e),
    }
}

async fn put_secret(State(k): K, h: HeaderMap, Path(name): Path<String>, Json(b): Json<Value>) -> Response {
    let p = match owner(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(value) = b["value"].as_str() else { return err(StatusCode::BAD_REQUEST, "no value") };
    if crate::secret_scope::personal(&k, &name) {
        return err(StatusCode::FORBIDDEN, crate::secret_scope::REFUSED);
    }
    if k.vault.is_sealed() {
        return err(StatusCode::CONFLICT, "the kill switch is engaged; turn it off before entering secrets");
    }
    if let Err(e) = k.vault.put(&name, value) {
        return internal(e);
    }
    match k.emit(&format!("person:{}", p.id), Ev::SecretSet { name }).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => internal(e),
    }
}

/// The kill switch and the secrets it took out (owner only): what to sign
/// in to or enter again, and whether each provider revoked it.
/// The backups on disk and whether the latest one restored (K10).
async fn backups(State(k): K, h: HeaderMap) -> Response {
    if let Err(r) = owner(&k, &h).await {
        return r;
    }
    Json(crate::backup::status(&k)).into_response()
}

async fn back_up(State(k): K, h: HeaderMap) -> Response {
    let p = match owner(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match k.back_up(&format!("person:{}", p.id)).await {
        Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
        Err(e) => internal(e),
    }
}

async fn kill_switch_state(State(k): K, h: HeaderMap) -> Response {
    if let Err(r) = owner(&k, &h).await {
        return r;
    }
    let (engaged, revoked, sealed) = k.read(|s| (s.kill_switch, s.revoked.clone(), s.sealed.clone()));
    Json(json!({ "engaged": engaged, "revoked": revoked, "sealed": if engaged { sealed } else { vec![] } })).into_response()
}

impl Kernel {
    /// The one kill switch (B11; owner's ruling 10): the app's and the admin
    /// console's are this. Engaged, it seals the vault, revokes every
    /// sign-in at its provider and suspends every running run; released,
    /// it tells each person what it stopped, and resumes nothing (K53).
    /// `actor` is who did it, as the log names them. The answer, or the
    /// status and body of a failure.
    pub async fn kill_switch(self: &Arc<Self>, actor: &str, engaged: bool) -> Result<Value, (StatusCode, Value)> {
        let k = self.clone();
        let actor = actor.to_string();
        // One at a time: a release racing an engage could unseal before the
        // engage sealed, or seal after the release.
        static SWITCHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _one = SWITCHING.lock().await;
        if !engaged {
            // Released: the vault takes secrets again, but holds none; nothing
            // resumes by itself (K53). Each person's tasks are their own: each
            // is told which of theirs the switch stopped, and continues them,
            // in the same write as the release, so none is released untold.
            let stopped: Vec<crate::state::Run> =
                k.read(|s| s.runs.values().filter(|r| r.status == Status::Suspended && r.reason.as_deref() == Some("kill_switch")).cloned().collect());
            let why = json!({ "reason": "kill_switch", "evidence": { "why": "the owner engaged the kill switch, and has released it; Continue goes on from where it stopped" } });
            let mut evs = vec![(actor.as_str(), Ev::KillSwitch { engaged })];
            for r in &stopped {
                // Background work is not the person's to continue: it ends,
                // and the next wake starts fresh (owner, 2026-11-07).
                if k.run_kind(&r.id).is_background() {
                    evs.push(("kernel", Ev::Failed { run: r.id.clone(), reason: "background work stopped by the kill switch".into(), by: "kernel".into() }));
                    continue;
                }
                let title = k.title_of(r).await;
                evs.push(("kernel", k.notice_of(r, format!("Stopped by the kill switch: {title}"), why.clone())));
            }
            if let Err(e) = k.emit_all(evs).await {
                return Err((StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": format!("{e:#}") })));
            }
            k.vault.unseal();
            k.release_computers(&actor, crate::halts::SERVER_REASON).await;
            for r in stopped.iter().filter(|r| k.run_kind(&r.id).is_background()) {
                k.on_stop(&r.id, true).await;
            }
            return Ok(json!({ "engaged": false, "told": stopped.iter().filter(|r| !k.run_kind(&r.id).is_background()).count() }));
        }
        // One control (B11). The vault is sealed first, and each OAuth sign-in
        // taken out of it and revoked at its provider, whatever happens to the
        // records after: the switch must work when the log cannot be written.
        // API keys and app secrets stay sealed: only their provider's console
        // can revoke them.
        k.vault.seal();
        // Every brokered connector's microVM, with whatever call it was making.
        k.brokered.stop(None, None).await;
        let names = match k.vault.names() {
            Ok(n) => n,
            Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": format!("{e:#}") }))),
        };
        let mut revoking = vec![];
        // Every person's own sign-in to every app people sign in to, turned on
        // or off: an app turned off keeps its sign-ins.
        let signed_in: Vec<String> = k.all_connectors().into_iter().map(|(c, _)| c).filter(|c| c.auth.as_deref() == Some(crate::connectors::OAUTH)).map(|c| c.name).collect();
        for n in &names {
            if let Some(adapter) = k.adapters.oauth_owner_of(n).await {
                revoking.push((n.clone(), adapter));
            } else if let Some(app) = signed_in.iter().find(|app| crate::connectors::is_sign_in_of(n, app)) {
                revoking.push((n.clone(), format!("connector:{app}")));
            }
        }
        let take: Vec<String> = revoking.iter().map(|(n, _)| n.clone()).collect();
        // With any an earlier switch took and a crash kept from revoking.
        let taken = match k.vault.seal_and_take(&take) {
            Ok(t) => t,
            Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": format!("{e:#}") }))),
        };
        let take: Vec<String> = taken.iter().map(|(n, _)| n.clone()).collect();
        // Revoked at once; each result is recorded only after the vault's
        // sealing below (or its failure), which starts the list it goes in: a
        // result recorded first was wiped, and the owner told it was not done.
        let (sealed_tx, sealed_rx) = tokio::sync::oneshot::channel::<()>();
        k.revoke_taken(taken, revoking, Some(sealed_rx));
        // Then the records: the policy is deny-all from this event on, and every
        // run is suspended.
        let kept: Vec<String> = names.iter().filter(|n| !take.contains(n)).cloned().collect();
        let recorded = async {
            k.emit(&actor, Ev::KillSwitch { engaged }).await?;
            let active: Vec<String> = k.read(|s| s.runs.values().filter(|r| r.status == Status::Running).map(|r| r.id.clone()).collect());
            for run in active {
                k.suspend(&run, "kill_switch", None).await?;
            }
            k.emit(&actor, Ev::VaultSealed { revoking: take.clone(), kept: kept.clone() }).await?;
            anyhow::Ok(())
        };
        let recorded = recorded.await;
        let _ = sealed_tx.send(());
        // Every run's pages in the browser, the running ones' and those already
        // waiting (a suspended run keeps its context otherwise), whether or not
        // the records were written. After the suspensions, so no run opens a
        // page after it; a run continued later starts on a blank one.
        k.browser.close_all().await;
        k.hold_computers(&actor, crate::halts::SERVER_REASON, None).await;
        if let Err(e) = recorded {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": format!("The vault is sealed and the sign-ins are being revoked, but recording the kill switch failed: {e:#}. Engage it again once the appliance can write."), "sealed": true }),
            ));
        }
        Ok(json!({ "engaged": true, "revoking": take, "sealed": kept }))
    }
}

impl Kernel {
    /// Revoke at their providers the sign-ins the kill switch took out of
    /// the vault, each let go by the vault once its provider has answered,
    /// and record each result once `after` says the sealing is recorded.
    fn revoke_taken(self: &Arc<Self>, taken: Vec<(String, crate::vault::Secret)>, known: Vec<(String, String)>, after: Option<tokio::sync::oneshot::Receiver<()>>) {
        let k = self.clone();
        tokio::spawn(async move {
            let mut done = vec![];
            for (name, secret) in taken {
                let adapter = match known.iter().find(|(n, _)| *n == name) {
                    Some((_, a)) => a.clone(),
                    None => k.revoker_of(&name).await,
                };
                let (at_provider, detail) = if adapter.starts_with("connector:") {
                    crate::connectors::revoke(&k, secret.expose()).await
                } else {
                    k.adapters.revoke(&adapter, secret.expose().to_string()).await
                };
                if let Err(e) = k.vault.revoked(&name) {
                    tracing::error!("could not let go of {name} after revoking it: {e:#}");
                }
                done.push(Ev::SecretRevoked { name, adapter, at_provider, detail });
            }
            if let Some(after) = after {
                let _ = after.await;
            }
            for ev in done {
                if let Err(e) = k.emit("kernel", ev).await {
                    tracing::error!("could not record a revocation: {e:#}");
                }
            }
        });
    }

    /// Who revokes a sign-in the switch took: its adapter, or the app it
    /// signs in to (`connector:<app>`), even one removed since.
    async fn revoker_of(&self, name: &str) -> String {
        if let Some(adapter) = self.adapters.oauth_owner_of(name).await {
            return adapter;
        }
        let app = self.all_connectors().into_iter().map(|(c, _)| c.name).find(|app| crate::connectors::is_sign_in_of(name, app));
        format!("connector:{}", app.unwrap_or_default())
    }

    /// At start: a kill switch a crash cut short is finished. Sealed but not
    /// recorded, it is engaged again (which records it and revokes what it
    /// took); otherwise what it took and did not see revoked is revoked,
    /// released since or not.
    pub async fn finish_kill_switch(self: &Arc<Self>) {
        if self.vault.is_sealed() && !self.read(|s| s.kill_switch) {
            tracing::warn!("the vault is sealed but the kill switch is not recorded: engaging it again");
            if let Err((_, e)) = self.kill_switch("kernel", true).await {
                tracing::error!("could not engage the kill switch again: {e}");
            }
            return;
        }
        match self.vault.revoking() {
            Ok(taken) if !taken.is_empty() => self.revoke_taken(taken, vec![], None),
            Ok(_) => {}
            Err(e) => tracing::error!("could not read the sign-ins waiting to be revoked: {e:#}"),
        }
    }
}

async fn kill_switch(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = match owner(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match k.kill_switch(&format!("person:{}", p.id), b["engaged"].as_bool().unwrap_or(true)).await {
        Ok(v) => Json(v).into_response(),
        Err((st, v)) => (st, Json(v)).into_response(),
    }
}

async fn adapters(State(k): K, h: HeaderMap) -> Response {
    if let Err(r) = owner(&k, &h).await {
        return r;
    }
    let list: Vec<Value> = k
        .adapters
        .list()
        .await
        .into_iter()
        .map(|(id, cmd, running)| {
            let verified = k.read(|s| s.conformance.get(&id).cloned());
            json!({
                "id": id, "command": cmd,
                "running": running.map(|(a, v)| json!({ "adapter": a, "version": v })),
                "conformance": verified.map(|(v, m, ok, _)| json!({ "version": v, "model": m, "passed": ok })),
            })
        })
        .collect();
    Json(json!({ "adapters": list })).into_response()
}

/// Replace or add an adapter while the kernel runs. The candidate must pass
/// the conformance suite against the live provider before it takes requests,
/// unless the owner explicitly skips the gate.
async fn put_adapter(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let p = match owner(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let cfg: Adapter = match serde_json::from_value(json!({ "id": id, "command": b["command"], "settings": b.get("settings").cloned().unwrap_or(json!({})) })) {
        Ok(c) => c,
        Err(e) => return err(StatusCode::BAD_REQUEST, format!("bad adapter: {e}")),
    };
    let candidate = match k.adapters.launch(&cfg).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    };
    let info = json!({ "adapter": candidate.adapter, "version": candidate.version });
    if b["skip_conformance"].as_bool() != Some(true) {
        let model = match conformance_model(&k, &id, &b["model"]).await {
            Ok(m) => m,
            Err(m) => return err(StatusCode::BAD_GATEWAY, m),
        };
        let report = k.adapters.conformance(&candidate, &model).await;
        let rv = serde_json::to_value(&report).unwrap_or_default();
        let ev = Ev::AdapterVerified { adapter_id: id.clone(), adapter: report.adapter.clone(), version: report.version.clone(), model, passed: report.passed, report: rv.clone() };
        if let Err(e) = k.emit(&format!("person:{}", p.id), ev).await {
            return internal(e);
        }
        if !report.passed {
            return (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "installed": false, "conformance": rv }))).into_response();
        }
    }
    k.adapters.install(candidate).await;
    Json(json!({ "installed": true, "running": info })).into_response()
}

/// The model a conformance run uses when none is named: the appliance's model
/// if this adapter serves it, otherwise the first model the adapter lists
/// (a fixed version first). Another provider's model would fail every check.
async fn conformance_model(k: &Kernel, id: &str, asked: &Value) -> Result<String, String> {
    if let Some(m) = asked.as_str() {
        return Ok(m.to_string());
    }
    let (adapter, model) = k.read(|s| s.model.clone()).unwrap_or_else(|| (k.cfg.model.adapter.clone(), k.cfg.model.name.clone()));
    if adapter == id {
        return Ok(model);
    }
    match k.adapters.catalog(id).await {
        Ok(magnus_adapter_proto::Event::Catalog { models }) => {
            models.iter().find(|m| m.fixed_version).or(models.first()).map(|m| m.id.clone()).ok_or_else(|| format!("adapter {id} lists no models; name one to test"))
        }
        Ok(magnus_adapter_proto::Event::Error { message, .. }) => Err(format!("could not list adapter {id}'s models: {message}")),
        Ok(other) => Err(format!("unexpected answer to the model list: {other:?}")),
        Err(e) => Err(format!("{e:#}")),
    }
}

async fn conformance(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let p = match owner(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let running = match k.adapters.get(&id).await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    };
    let model = match conformance_model(&k, &id, &b["model"]).await {
        Ok(m) => m,
        Err(m) => return err(StatusCode::BAD_GATEWAY, m),
    };
    let report = k.adapters.conformance(&running, &model).await;
    let rv = serde_json::to_value(&report).unwrap_or_default();
    let ev = Ev::AdapterVerified { adapter_id: id, adapter: report.adapter.clone(), version: report.version.clone(), model, passed: report.passed, report: rv.clone() };
    if let Err(e) = k.emit(&format!("person:{}", p.id), ev).await {
        return internal(e);
    }
    Json(rv).into_response()
}

async fn catalog(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    if let Err(r) = owner(&k, &h).await {
        return r;
    }
    match k.adapters.catalog(&id).await {
        Ok(ev) => Json(serde_json::to_value(ev).unwrap_or_default()).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

async fn sign_in_begin(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    if let Err(r) = owner(&k, &h).await {
        return r;
    }
    match k.adapters.auth_begin(&id).await {
        Ok(ev) => Json(serde_json::to_value(ev).unwrap_or_default()).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

async fn sign_in_complete(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let p = match owner(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match k.adapters.auth_complete(&id, b["flow"].clone(), b["input"].clone()).await {
        Ok(Ok(())) => {
            let name = k.cfg.adapters.iter().find(|a| a.id == id).and_then(|a| a.settings.get("credential")).and_then(|v| v.as_str()).unwrap_or(&id).to_string();
            let _ = k.emit(&format!("person:{}", p.id), Ev::SecretSet { name }).await;
            Json(json!({ "ok": true })).into_response()
        }
        Ok(Err(m)) => err(StatusCode::BAD_REQUEST, m),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

async fn memory(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    // Recalled for the person, through their wall.
    let items: Vec<Value> = match k.remembered(&p.id, |_| true).await {
        Ok(r) => r
            .into_iter()
            .map(|r| {
                let m = r.entry;
                json!({ "id": m.id, "fact": r.fact, "topic": r.topic, "label": m.label, "run": m.run, "by": m.by.as_deref().unwrap_or("model"), "at": m.at })
            })
            .collect(),
        Err(e) => return internal(e),
    };
    Json(json!({ "memory": items })).into_response()
}

/// Forget one of the person's own memories.
async fn forget(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if !k.read(|s| s.memory.iter().any(|m| m.id == id && m.person == p.id)) {
        return err(StatusCode::NOT_FOUND, "no such memory");
    }
    crate::trash::trashed(k.trash(&p.id, p.is_owner(), &id).await)
}

async fn health(State(k): K) -> Response {
    // Down while the log's connection is lost: nothing can be recorded.
    let log = k.log.up().await;
    let body = Json(
        json!({ "ok": log, "log": if log { "up" } else { "down" }, "version": HARNESS_VERSION, "release": crate::update::version(), "kill_switch": k.read(|s| s.kill_switch) }),
    );
    if log {
        body.into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, body).into_response()
    }
}

/// Model text for a task as it streams (server-sent events), before the
/// turn is appended to the log (K6).
async fn live(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some((_, run)) = find(&k, &p, &id) else { return err(StatusCode::NOT_FOUND, "no such task") };
    let (so_far, mut rx) = k.live_text.subscribe(&run.id);
    let stream = async_stream(move |tx| async move {
        if !so_far.is_empty() && tx.send(format!("data: {}\n\n", json!({ "text": so_far }))).await.is_err() {
            return;
        }
        loop {
            match rx.recv().await {
                Ok((r, text)) if r == run.id => {
                    if tx.send(format!("data: {}\n\n", json!({ "text": text }))).await.is_err() {
                        return;
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => return,
            }
        }
    });
    Response::builder().header("content-type", "text/event-stream").header("cache-control", "no-cache").body(axum::body::Body::from_stream(stream)).unwrap()
}

fn async_stream<F, Fut>(f: F) -> impl futures::Stream<Item = Result<String, std::convert::Infallible>>
where
    F: FnOnce(tokio::sync::mpsc::Sender<String>) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(f(tx));
    futures::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|s| (Ok(s), rx)) })
}

/// Speech to text on the appliance. The person reviews the text before sending it.
async fn transcribe(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if let Err(r) = who(&k, &h).await {
        return r;
    }
    let Some(data) = b["data_b64"].as_str() else { return err(StatusCode::BAD_REQUEST, "no audio") };
    match crate::speech::transcribe(k.cfg.speech.as_ref(), &k.cfg.appliance.state_dir, data).await {
        Ok(text) => Json(json!({ "text": text })).into_response(),
        Err(e) => err(StatusCode::SERVICE_UNAVAILABLE, format!("{e:#}")),
    }
}

/// Text to speech on the appliance, so every app and browser hears the same
/// voice. 503 when no voice is installed: the app then uses its system's own.
async fn speak(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if let Err(r) = who(&k, &h).await {
        return r;
    }
    let Some(cfg) = &k.cfg.read_aloud else { return err(StatusCode::SERVICE_UNAVAILABLE, "no read-aloud voice is installed on this server") };
    let Some(text) = b["text"].as_str().filter(|t| !t.trim().is_empty()) else { return err(StatusCode::BAD_REQUEST, "no text") };
    use crate::speech::SpeakError;
    match crate::speech::speak_limited(cfg, text).await {
        Ok(mp3) => Json(json!({ "media_type": "audio/mpeg", "audio_b64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, mp3) })).into_response(),
        Err(SpeakError::TooLong) => err(StatusCode::PAYLOAD_TOO_LARGE, format!("read aloud takes at most {} characters at a time", crate::speech::MAX_SPEAK)),
        Err(SpeakError::Busy) => err(StatusCode::TOO_MANY_REQUESTS, "the voice is busy; try again in a moment"),
        Err(SpeakError::TimedOut) => err(StatusCode::GATEWAY_TIMEOUT, "reading this aloud took too long"),
        Err(SpeakError::Failed(e)) => {
            tracing::warn!("read aloud: {e:#}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "read aloud failed")
        }
    }
}

async fn get_model(State(k): K, h: HeaderMap) -> Response {
    if let Err(r) = who(&k, &h).await {
        return r;
    }
    let (adapter, model) = k.read(|s| s.model.clone()).unwrap_or_else(|| (k.cfg.model.adapter.clone(), k.cfg.model.name.clone()));
    Json(json!({ "adapter": adapter, "model": model })).into_response()
}

/// The models anyone signed in may choose for a task or a chat message,
/// and the one used when they choose none.
async fn models(State(k): K, h: HeaderMap) -> Response {
    let Ok(p) = who(&k, &h).await else { return err(StatusCode::UNAUTHORIZED, "sign in") };
    let (default, lists) = k.models_for(&p.id).await;
    let list: Vec<Value> = lists.into_iter().map(|(id, models)| json!({ "adapter": id, "models": models })).collect();
    Json(json!({ "default": default.map(|(adapter, model)| json!({ "adapter": adapter, "model": model })), "adapters": list })).into_response()
}

/// Choose the model new tasks run on. Running tasks keep their pinned model (K9).
async fn put_model(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = match owner(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let (Some(adapter), Some(model)) = (b["adapter"].as_str(), b["model"].as_str()) else {
        return err(StatusCode::BAD_REQUEST, "adapter and model are required");
    };
    if k.adapters.list().await.iter().all(|(id, _, _)| id != adapter) {
        return err(StatusCode::NOT_FOUND, "no such adapter");
    }
    match k.emit(&format!("person:{}", p.id), Ev::ModelSet { adapter: adapter.into(), model: model.into() }).await {
        Ok(_) => Json(json!({ "adapter": adapter, "model": model })).into_response(),
        Err(e) => internal(e),
    }
}

async fn schedules(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let words = match k.person_words(&p.id).await {
        Ok(w) => w,
        Err(e) => return internal(e),
    };
    let list: Vec<Value> = k.read(|s| {
        s.schedules
            .values()
            .filter(|x| x.person == p.id)
            .map(|x| {
                // Its runs, newest first; the last result is the newest that ended.
                let mut runs: Vec<(&crate::state::Task, &Run)> =
                    s.tasks.values().filter(|t| t.schedule.as_deref() == Some(x.id.as_str())).filter_map(|t| s.runs.get(&t.run_id).map(|r| (t, r))).collect();
                runs.sort_by(|a, b| b.0.created_at.cmp(&a.0.created_at));
                let rows: Vec<Value> = runs.iter().map(|(t, r)| task_row(&k, s, &p, t, r, &words)).collect();
                // Every firing shows: the newest task whatever its state, and firings that could not start one.
                let last = rows.first().cloned();
                let failures: Vec<Value> = x.failures.iter().rev().map(|(at, why)| json!({ "at": at, "error": why })).collect();
                let last_failed = x
                    .failures
                    .last()
                    .filter(|(at, _)| rows.first().and_then(|r| r["created_at"].as_str()).and_then(|c| chrono::DateTime::parse_from_rfc3339(c).ok()).is_none_or(|c| c < *at))
                    .map(|(at, why)| json!({ "at": at, "error": why }));
                json!({
                    "id": x.id, "person": x.person, "text": words.text(x.seq), "title": words.title(x.seq).unwrap_or_else(|| words.text(x.seq).to_string()),
                    "every_minutes": x.every_minutes, "daily_at": x.daily_at, "next": x.next_due(), "last_fired": x.last_fired,
                    "until": x.until, "max_runs": x.max_runs, "started": x.started, "paused": x.paused, "paused_on": x.paused_on, "ended": x.ended,
                    // Each run's limit: the server's per-task budget.
                    "budget": crate::task_budget::scheduled(&k.cfg.budget, x.budget.as_ref()),
                    "skipped": x.skipped.as_ref().map(|(at, task, why)| json!({ "at": at, "task": task, "why": why })),
                    "last": last, "last_failed": last_failed, "failures": failures, "runs": rows,
                    "project": s.project_of(&x.id).filter(|_| x.person == p.id).and_then(|id| s.projects.get(&id)).map(|p| json!({ "id": p.id, "name": p.name })),
                })
            })
            .collect()
    });
    Json(json!({ "schedules": list })).into_response()
}

fn schedule_rule(b: &Value, min: u32) -> Result<(Option<u32>, Option<String>), String> {
    // A model that must fill every field sends 0 or "" for the one it does not mean.
    let every = b["every_minutes"].as_u64().filter(|m| *m > 0).map(|m| m as u32);
    let daily = b["daily_at"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(String::from);
    match (every, &daily) {
        (Some(m), None) if m >= min => Ok((Some(m), None)),
        (Some(_), None) => Err(format!("schedules run at most every {min} minutes")),
        (None, Some(t)) if t.split_once(':').is_some_and(|(h, m)| h.parse::<u32>().is_ok_and(|h| h < 24) && m.parse::<u32>().is_ok_and(|m| m < 60)) => Ok((None, daily)),
        _ => Err("give either every_minutes or daily_at as HH:MM".into()),
    }
}

async fn add_schedule(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let text = b["text"].as_str().unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return err(StatusCode::BAD_REQUEST, "the schedule has no task text");
    }
    let (every_minutes, daily_at) = match schedule_rule(&b, k.cfg.schedules.min_every_minutes) {
        Ok(r) => r,
        Err(m) => return err(StatusCode::BAD_REQUEST, m),
    };
    let (until, max_runs) = match end_for(&b) {
        Ok(r) => r,
        Err(m) => return err(StatusCode::BAD_REQUEST, m),
    };
    let project = match crate::library::chosen_project(&k, &p, &b) {
        Ok(x) => x,
        Err(r) => return r,
    };
    let id = new_id("sched");
    let actor = format!("person:{}", p.id);
    if let Err(e) = k
        .emit(
            &actor,
            Ev::ScheduleSet {
                id: id.clone(),
                person: p.id.clone(),
                text,
                every_minutes,
                daily_at,
                budget: None,
                card: None,
                label: None,
                title: None,
                until,
                max_runs,
                scope: None,
                // Set up in settings, outside a conversation: its own thread.
                thread: None,
            },
        )
        .await
    {
        return internal(e);
    }
    if project.is_some() {
        if let Err(e) = k.emit(&actor, Ev::Moved { item: id.clone(), project }).await {
            return internal(e);
        }
    }
    (StatusCode::CREATED, Json(json!({ "id": id }))).into_response()
}

/// Start a paused schedule again: failures count afresh, and the next run
/// is due one interval from now.
async fn resume_schedule(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match controlled(k.resume_schedule(&p, &id, None).await.map(|_| Value::Null)) {
        r if r.status() == StatusCode::OK => StatusCode::NO_CONTENT.into_response(),
        r => r,
    }
}

/// Pause: no more runs until it is resumed.
async fn pause_schedule(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match controlled(k.pause_schedule(&p, &id, None).await.map(|_| Value::Null)) {
        r if r.status() == StatusCode::OK => StatusCode::NO_CONTENT.into_response(),
        r => r,
    }
}

/// Change when it runs and when it ends.
async fn edit_schedule(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    controlled(k.edit_schedule(&p, &id, &b, None).await)
}

/// Cancel: no more runs, and one still going stops.
async fn remove_schedule(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    // The report goes back with the status: what each stopped run had
    // done, and how many actually stopped (#263).
    controlled(k.cancel_schedule(&p, &id, None).await)
}

pub(crate) fn rule_for(b: &Value, min: u32) -> Result<(Option<u32>, Option<String>), String> {
    schedule_rule(b, min)
}

/// When a schedule ends: `until` (RFC 3339) and `max_runs`, either, both or
/// neither. A model that must fill every field sends "" or 0 for one it
/// does not mean.
pub(crate) fn end_for(b: &Value) -> Result<(Option<chrono::DateTime<chrono::Utc>>, Option<u32>), String> {
    let until = match b["until"].as_str().map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) => Some(chrono::DateTime::parse_from_rfc3339(u).map_err(|e| format!("until is not an RFC 3339 time ({e})"))?.with_timezone(&chrono::Utc)),
        None => None,
    };
    if until.is_some_and(|u| u <= chrono::Utc::now()) {
        return Err("until is already past".into());
    }
    let max_runs = b["max_runs"].as_u64().filter(|m| *m > 0).map(|m| u32::try_from(m).unwrap_or(u32::MAX));
    Ok((until, max_runs))
}

/// Download a delivered file. Read from the log, the source of truth.
async fn output(State(k): K, h: HeaderMap, Path((id, out)): Path<(String, String)>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some((_, run)) = find(&k, &p, &id) else { return err(StatusCode::NOT_FOUND, "no such task") };
    if !run.outputs.iter().any(|(o, ..)| o == &out) {
        return err(StatusCode::NOT_FOUND, "no such file");
    }
    match k.output_bytes(&run.id, &out).await {
        Ok(Some((name, mt, bytes))) => crate::library::attachment(&name, &mt, bytes),
        Ok(None) => err(StatusCode::NOT_FOUND, "no such file"),
        Err(e) => internal(e),
    }
}
