use super::{credential, err, internal, refused_by_kill_switch, who, Transport, K, SESSION_COOKIE};
use crate::api_error::{refuse, ApiResult};
use crate::extract::{Body, Caller};
use crate::{api_bodies as req, events::Ev, kernel::Kernel, people::rights};
use axum::{
    extract::{ConnectInfo, Form, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::Arc};

/// Sign-in hands out a session, so over plain HTTP only from this machine:
/// neither a password nor a session crosses the network unencrypted.
pub(crate) fn over_tls(k: &Kernel, t: Transport, peer: SocketAddr) -> Result<(), Box<Response>> {
    if t == Transport::Plain && !peer.ip().is_loopback() {
        let hint = match &k.cfg.appliance.tls_listen {
            Some(l) => format!("sign in over https (port {})", l.rsplit(':').next().unwrap_or_default()),
            None => "sign in over https; the owner has not turned it on (appliance.tls_listen)".into(),
        };
        return Err(Box::new(err(StatusCode::FORBIDDEN, hint)));
    }
    Ok(())
}

/// Open a session for a person and hand it over: to the app as a token,
/// to a browser as an HttpOnly cookie the page cannot read.
pub(crate) async fn issue(k: &Arc<Kernel>, t: Transport, person: &str, app: bool, device: Option<&str>) -> Response {
    if let Err(e) = k.accounts.refresh_people().await {
        return internal(e);
    }
    if let Some(r) = turned_off(k, person).await {
        return r;
    }
    let Some(p) = k.person(person) else { return err(StatusCode::FORBIDDEN, format!("{person} is not a person on this server.")) };
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

/// A refusal for someone whose account here is turned off, whoever vouches
/// for them: nothing of the sign-in is recorded, so their work stays held
/// (decision `2026-11-23-one-person-is-turned-off.md`).
async fn turned_off(k: &Kernel, person: &str) -> Option<Response> {
    (k.accounts.status(person).await == crate::accounts::Status::Disabled).then(|| err(StatusCode::FORBIDDEN, "Your account here is turned off. Ask an admin to turn it back on."))
}

/// How people sign in here: through the identity provider when one is
/// configured, with a password always open as the way in when it is down.
pub(super) async fn sign_in_method(State(k): K) -> ApiResult {
    let mut body = match k.oidc.get() {
        Some(o) => json!({ "method": "oidc", "label": o.label(), "password": true }),
        None => json!({ "method": "password", "password": true }),
    };
    if let Some(c) = &k.cfg.client_certs {
        let host = c.hostname.trim().trim_start_matches("https://").trim_end_matches('/');
        body["public_url"] = json!(format!("https://{host}"));
        body["device_certificates"] = json!(true);
    }
    Ok(Json(body).into_response())
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

fn cloudflare_of(k: &Kernel) -> ApiResult<crate::devices::Cloudflare> {
    let Some(cfg) = k.cfg.client_certs.clone() else { return refuse(StatusCode::NOT_FOUND, "this server does not issue device certificates") };
    if k.vault.is_sealed() {
        return refuse(StatusCode::CONFLICT, "The kill switch is on, so a device certificate cannot be issued or revoked at Cloudflare until the owner releases it.");
    }
    let token = match k.vault.get(&cfg.credential) {
        Ok(Some(s)) => s.expose().to_string(),
        Ok(None) => return refuse(StatusCode::SERVICE_UNAVAILABLE, format!("The vault has no secret {} for device certificates.", cfg.credential)),
        Err(e) => return Err(e.into()),
    };
    Ok(crate::devices::Cloudflare { api: crate::devices::CF_API.into(), zone: cfg.zone_id, token, days: cfg.days })
}

/// Pair this computer. The body is the certificate request the computer
/// made; the private key stays there. Refused through the tunnel, so the
/// answer never crosses Cloudflare: pair on the appliance's own network,
/// then the public name accepts the certificate.
pub(super) async fn enroll_device(State(k): K, ConnectInfo(peer): ConnectInfo<SocketAddr>, h: HeaderMap, Body(b): Body<req::Enroll>) -> ApiResult {
    if k.cfg.client_certs.is_none() {
        return refuse(StatusCode::NOT_FOUND, "this server does not issue device certificates");
    }
    if crate::devices::is_tunnel(peer, &h) {
        return refuse(
            StatusCode::FORBIDDEN,
            "Pair this computer from the server's own network. The public address accepts a computer that is already paired and does not hand out a certificate.",
        );
    }
    if let Some(r) = refused_by_kill_switch(&k) {
        return Ok(r);
    }
    let person = who(&k, &h).await?;
    let cf = cloudflare_of(&k)?;
    let issued = match k.devices.issue(&cf, &person.id, &b.name, &b.csr).await {
        Ok(i) => i,
        Err(e) => return Ok(device_error(e)),
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
    Ok(Json(json!({
        "device": issued.device,
        "fingerprint": issued.fingerprint,
        "certificate": issued.certificate,
        "public_url": format!("https://{host}"),
    }))
    .into_response())
}

pub(super) async fn list_devices(State(k): K, Caller(person): Caller) -> ApiResult {
    let mine = if rights(&person).manages_every_device() { None } else { Some(person.id.as_str()) };
    let devices: Vec<Value> = k
        .devices
        .list(mine)
        .into_iter()
        .map(|d| json!({ "id": d.id, "person": d.person, "name": d.name, "created": d.created, "revoked": d.revoked, "fingerprint": d.fingerprint }))
        .collect();
    Ok(Json(json!({ "devices": devices })).into_response())
}

pub(super) async fn revoke_device(State(k): K, Caller(person): Caller, Path(id): Path<String>) -> ApiResult {
    let Some(device) = k.devices.get(&id) else { return refuse(StatusCode::NOT_FOUND, "no such device") };
    if device.person != person.id && !rights(&person).manages_every_device() {
        return refuse(StatusCode::NOT_FOUND, "no such device");
    }
    let cf = cloudflare_of(&k)?;
    if let Err(e) = k.devices.revoke(&cf, &id).await {
        return Ok(device_error(e));
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
    Ok(Json(json!({ "ok": true })).into_response())
}

const SIGN_IN_BEGIN_WINDOW_SECS: std::time::Duration = std::time::Duration::from_secs(10 * 60);
/// Sign-ins begun from one address in the last ten minutes: at most a
/// quarter of the provider's waiting ones (`max_pending`), which everyone
/// shares, so no one address can fill them and stop every first sign-in.
fn may_begin(k: &Kernel, from: std::net::IpAddr) -> bool {
    let most = k.cfg.sign_in.oidc.as_ref().map_or(25, |o| (o.max_pending / 4).max(5));
    static BEGUN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<std::net::IpAddr, Vec<std::time::Instant>>>> = std::sync::OnceLock::new();
    let mut m = BEGUN.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()); // a panic elsewhere leaves counts that are still counts
    m.retain(|_, v| {
        v.retain(|t| t.elapsed() < SIGN_IN_BEGIN_WINDOW_SECS);
        !v.is_empty()
    });
    let v = m.entry(from).or_default();
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
/// Without one, only the appliance's own passwords. The check and its
/// lockout are `password_check`'s; this only answers.
pub(super) async fn sign_in(State(k): K, Extension(t): Extension<Transport>, ConnectInfo(peer): ConnectInfo<SocketAddr>, h: HeaderMap, Body(b): Body<req::SignIn>) -> ApiResult {
    use crate::password_check::{Attempt, Checked, Via};
    over_tls(&k, t, peer)?;
    let person = crate::sign_in_name::resolve(&k.accounts, &b.person).await;
    let password = b.password.as_str();
    let code = b.code.as_deref().map(str::trim).filter(|c| !c.is_empty());
    let machine_code = b.machine_code.as_deref().map(str::trim).filter(|c| !c.is_empty());
    let app = b.client.as_deref() == Some("app");
    if let Some(r) = app.then(|| crate::update::unversioned_app(&h)).flatten() {
        return Ok(r);
    }
    let device = device_of(&k, peer, &h);
    let via = crate::password_check::via(peer, &h);
    let provider = crate::password_check::provider_for(&k, &person).await;
    let label = provider.as_ref().map(|i| i.label()).unwrap_or_default();
    // The first time, the provider's own page signs them in: it can walk
    // them through what a new account needs.
    if provider.is_some() && machine_code.is_none() && !k.read(|s| s.identity.signed_in_before(&person)) {
        let page = crate::identity::Polled::UseBrowser(format!("The first time, sign in on the {label} page that just opened in your browser."));
        return Ok(provider_answered(&k, t, label, page, device).await);
    }
    let wrong = if provider.is_some() && machine_code.is_some() { "The name, password or code is wrong." } else { "The name or password is wrong." };
    let attempt = Attempt { person: &person, password, code, machine_code, via, app };
    Ok(match crate::password_check::check(&k, &attempt).await {
        Ok(Checked::Yes(Some(polled)) | Checked::Elsewhere(polled)) => provider_answered(&k, t, label, polled, device).await,
        Ok(Checked::Yes(None)) => issue(&k, t, &person, app, device.as_deref()).await,
        Ok(Checked::NeedsCode) => code_needed(),
        Ok(Checked::MustChoose) => (StatusCode::UNAUTHORIZED, Json(json!({ "must_choose": true, "error": "Choose your own password first." }))).into_response(),
        Ok(Checked::Wrong(m)) => err(StatusCode::UNAUTHORIZED, m.unwrap_or_else(|| wrong.into())),
        Ok(Checked::Locked(until)) => crate::password_check::locked(until, None),
        Ok(Checked::Unavailable(label)) => {
            // The owner's backup, offered only where it works.
            let backup = k.may_use_password(&person) && k.accounts.has_password(&person).await && via == Via::ThisMachine;
            if backup {
                return Ok((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({ "machine_code_needed": true, "error": format!("{label} is not answering. On the server, run `magnus sign-in-code` and enter the code it prints, with your password.") })),
                )
                    .into_response());
            }
            err(StatusCode::BAD_GATEWAY, format!("{label} is not answering. Try again in a few minutes."))
        }
        Err(e) => internal(e),
    })
}

/// Choose the permanent password with a temporary one: the console's
/// one-time password, or any temp the owner set. A wrong temp answers
/// as a wrong password; choosing activates, as accepting an invite
/// does. No session opens here — they sign in after, with the new one.
pub(super) async fn choose_password(
    State(k): K,
    Extension(t): Extension<Transport>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Body(b): Body<req::NewPassword>,
) -> ApiResult {
    over_tls(&k, t, peer)?;
    let person = crate::sign_in_name::resolve(&k.accounts, &b.person).await;
    let temp = b.password.as_str();
    let password = b.new_password.as_str();
    if password.chars().count() < k.cfg.sign_in.min_password_length {
        return refuse(StatusCode::BAD_REQUEST, format!("a password needs at least {} characters", k.cfg.sign_in.min_password_length));
    }
    Ok(match k.accounts.choose_password(&person, temp, password, crate::password_check::via(peer, &h)).await {
        Ok(Ok(_)) => Json(json!({ "person": person })).into_response(),
        Ok(Err(crate::accounts::SignInError::Locked(until))) => crate::password_check::locked(until, None),
        Ok(Err(_)) => err(StatusCode::UNAUTHORIZED, "The name or password is wrong."),
        Err(e) => internal(e),
    })
}

/// Ask for the 6-digit code from the person's authenticator app: the same
/// screen whether the identity provider or the local store asked (#429).
fn code_needed() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "code_needed": true, "error": "Enter the 6-digit code from your authenticator app." }))).into_response()
}

/// Start a sign-in through the identity provider: the person approves the
/// code on the provider's page, then polls `/v1/sign-in/device/{flow}`.
pub(super) async fn device_start(
    State(k): K,
    Extension(t): Extension<Transport>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Body(b): Body<req::Client>,
) -> ApiResult {
    over_tls(&k, t, peer)?;
    if let Some(r) = (b.client.as_deref() == Some("app")).then(|| crate::update::unversioned_app(&h)).flatten() {
        return Ok(r);
    }
    let Some(o) = k.oidc.get() else { return refuse(StatusCode::NOT_FOUND, "this server signs in with a password") };
    if !may_begin(&k, crate::devices::client_ip(peer, &h)) {
        return Ok(too_many_begins());
    }
    Ok(match o.start(b.client.as_deref() == Some("app")).await {
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
    })
}

pub(super) async fn device_poll(State(k): K, Extension(t): Extension<Transport>, ConnectInfo(peer): ConnectInfo<SocketAddr>, h: HeaderMap, Path(flow): Path<String>) -> ApiResult {
    over_tls(&k, t, peer)?;
    let Some(o) = k.oidc.get() else { return refuse(StatusCode::NOT_FOUND, "this server signs in with a password") };
    Ok(match o.poll(&flow).await {
        Ok(polled) => provider_answered(&k, t, o.label(), polled, device_of(&k, peer, &h)).await,
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    })
}

/// What the identity provider answered about a sign-in: wait, a session for
/// a person of this appliance with the rights the provider gives, or no.
async fn provider_answered(k: &Arc<Kernel>, t: Transport, label: String, polled: crate::identity::Polled, device: Option<String>) -> Response {
    use crate::identity::Polled;
    match polled {
        Polled::Pending => (StatusCode::ACCEPTED, Json(json!({ "pending": true }))).into_response(),
        Polled::Done { person, app, rights } => {
            if k.cfg.person(&person).is_none() {
                tracing::warn!("identity: {person} signed in at {label} but is not a person on this server");
                return err(StatusCode::FORBIDDEN, format!("You signed in as {person}, who is not a person on this server. Ask the owner to add you."));
            }
            if let Some(r) = turned_off(k, &person).await {
                tracing::warn!("identity: {person} signed in at {label} but is turned off here; refused");
                return r;
            }
            // A person is the provider's account that first signed in as
            // them (its `sub`), not a username: an account made again, or
            // renamed, under the same name is someone else (owner's
            // rulings, 2026-11-01).
            if let Some(sub) = rights.subject.clone() {
                let _decide = k.decide.lock().await;
                match k.read(|s| s.identity.subject(&person).map(str::to_string)) {
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
                let ev = Ev::RightsFromProvider(crate::events::RightsFromProviderPayload { person: person.clone(), groups: rights.groups, roles: rights.roles });
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
pub(super) async fn browser_begin(
    State(k): K,
    Extension(t): Extension<Transport>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Body(b): Body<req::BrowserBegin>,
) -> ApiResult {
    over_tls(&k, t, peer)?;
    if let Some(r) = (b.client.as_deref() == Some("app")).then(|| crate::update::unversioned_app(&h)).flatten() {
        return Ok(r);
    }
    let Some(id) = k.identity() else { return refuse(StatusCode::NOT_FOUND, "this server signs in with a password") };
    let Some(redirect_uri) = b.redirect_uri.as_deref() else { return refuse(StatusCode::BAD_REQUEST, "name the redirect address") };
    if !may_begin(&k, crate::devices::client_ip(peer, &h)) {
        return Ok(too_many_begins());
    }
    if let Err(e) = crate::oauth::loopback(redirect_uri) {
        return refuse(StatusCode::BAD_REQUEST, format!("{e:#}"));
    }
    let hint = b.login_hint.as_deref().map(str::trim).filter(|h| !h.is_empty());
    Ok(match id.begin(redirect_uri, hint, b.client.as_deref() == Some("app")).await {
        Ok(s) => Json(json!({ "state": s.state, "authorize_url": s.authorize_url, "expires_in": s.expires_in, "label": id.label() })).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    })
}

/// Finish a browser sign-in with what came back to the loopback address.
pub(super) async fn browser_finish(
    State(k): K,
    Extension(t): Extension<Transport>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Body(b): Body<req::BrowserFinish>,
) -> ApiResult {
    over_tls(&k, t, peer)?;
    let Some(id) = k.identity() else { return refuse(StatusCode::NOT_FOUND, "this server signs in with a password") };
    let (Some(state), Some(code)) = (b.state.as_deref(), b.code.as_deref()) else { return refuse(StatusCode::BAD_REQUEST, "send the state and code that came back") };
    Ok(match id.finish(state, code, b.iss.as_deref()).await {
        Ok(polled) => provider_answered(&k, t, id.label(), polled, device_of(&k, peer, &h)).await,
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    })
}

/// Accept an invitation: the token with the new password activates the
/// person, who then signs in normally. A bad, used or expired token, or
/// one for someone no longer here, answers alike.
pub(super) async fn accept_invite(State(k): K, Extension(t): Extension<Transport>, ConnectInfo(peer): ConnectInfo<SocketAddr>, Body(b): Body<req::AcceptInvite>) -> ApiResult {
    over_tls(&k, t, peer)?;
    let token = b.token.as_str();
    let password = b.password.as_str();
    if password.chars().count() < k.cfg.sign_in.min_password_length {
        return refuse(StatusCode::BAD_REQUEST, format!("a password needs at least {} characters", k.cfg.sign_in.min_password_length));
    }
    Ok(match k.accept_invitation(token, password).await.map(|a| a.map(|a| a.person)) {
        Ok(Some(person)) if k.people().exists(&person) => {
            k.computer_soon(crate::computer::Owner::Person(person.clone()));
            Json(json!({ "person": person })).into_response()
        }
        Ok(_) => err(StatusCode::UNAUTHORIZED, "The invitation is wrong or expired."),
        Err(e) => internal(e),
    })
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
pub(super) async fn reset_page(
    State(k): K,
    Extension(t): Extension<Transport>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> ApiResult {
    over_tls(&k, t, peer)?;
    let token = q.get("token").map(|s| s.trim().to_string()).unwrap_or_default();
    if token.is_empty() || !k.accounts.reset_live(&token).await {
        return Ok(reset_shell("Link expired", "<h1>This recovery link is spent or expired.</h1><p>Ask for a new email and follow its link within the hour.</p>"));
    }
    Ok(reset_shell(
        "Choose a new password",
        &format!(
            "<h1>Choose a new password</h1><p>It signs every device out, and any authenticator codes stop working until new ones are enrolled.</p><form method=post action=/reset><input type=hidden name=token value=\"{}\"><label>New password<input type=password name=password autocomplete=new-password required></label><label>Repeat it<input type=password name=confirm autocomplete=new-password required></label><button>Set password</button></form>",
            h_escape(&token)
        ),
    ))
}

/// Set the password from the recovery page.
pub(super) async fn reset_confirm(
    State(k): K,
    Extension(t): Extension<Transport>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Form(f): Form<std::collections::HashMap<String, String>>,
) -> ApiResult {
    over_tls(&k, t, peer)?;
    let token = f.get("token").map(|s| s.trim().to_string()).unwrap_or_default();
    let password = f.get("password").cloned().unwrap_or_default();
    if f.get("confirm").map(|s| s.as_str()) != Some(password.as_str()) {
        return Ok(reset_shell("Choose a new password", &format!("<h1>Choose a new password</h1><p class=error>The two passwords do not match.</p><form method=post action=/reset><input type=hidden name=token value=\"{}\"><label>New password<input type=password name=password autocomplete=new-password required></label><label>Repeat it<input type=password name=confirm autocomplete=new-password required></label><button>Set password</button></form>", h_escape(&token))));
    }
    if password.chars().count() < k.cfg.sign_in.min_password_length {
        return Ok(reset_shell("Choose a new password", &format!("<h1>Choose a new password</h1><p class=error>A password needs at least {} characters.</p><form method=post action=/reset><input type=hidden name=token value=\"{}\"><label>New password<input type=password name=password autocomplete=new-password required></label><label>Repeat it<input type=password name=confirm autocomplete=new-password required></label><button>Set password</button></form>", k.cfg.sign_in.min_password_length, h_escape(&token))));
    }
    Ok(match k.accounts.confirm_reset(&token, &password).await {
        Ok(Some((person, ended))) => {
            let mut evs = vec![(person.as_str(), Ev::PasswordChanged { person: person.clone(), how: "reset".into() })];
            evs.extend(ended.into_iter().map(|session| (person.as_str(), Ev::SignedOut { person: person.clone(), session })));
            k.emit_all(evs).await?;
            reset_shell(
                "Password set",
                &format!("<h1>The new password is set.</h1><p>Sign in as <b>{}</b> or with your email address. Every device is signed out. If codes were enrolled, they no longer work: enroll new ones after signing in.</p>", h_escape(&person)),
            )
        }
        Ok(None) => reset_shell("Link expired", "<h1>This recovery link is spent or expired.</h1><p>Ask for a new email and follow its link within the hour.</p>"),
        Err(e) => internal(e),
    })
}

pub(super) async fn sign_out(State(k): K, h: HeaderMap) -> ApiResult {
    let Some(token) = credential(&h) else { return refuse(StatusCode::UNAUTHORIZED, "not signed in") };
    match k.accounts.sign_out(token).await {
        Ok(Some(s)) => {
            k.emit(&format!("person:{}", s.person), Ev::SignedOut { person: s.person.clone(), session: s.id }).await?;
        }
        Ok(None) => {}
        Err(e) => return Ok(internal(e)),
    }
    let mut r = Json(json!({ "ok": true })).into_response();
    if let Ok(v) = format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0").parse() {
        r.headers_mut().insert("set-cookie", v);
    }
    Ok(r)
}

#[cfg(test)]
#[path = "sign_in_off_tests.rs"]
mod off_tests;
