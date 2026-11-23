//! The HTTP API, for people's apps. Every request names a person: by a
//! session from sign-in (the desktop app sends it as a bearer token,
//! a browser as an HttpOnly cookie) or by a configured API token. Approvals,
//! answers and policy arrive only here, a channel the agent cannot reach (its
//! sandbox has no network and no tool calls this API).

mod owner;
mod sign_in;
mod work;
#[cfg(test)]
mod work_list_tests;

use crate::api_error::{refuse, session_unread, ApiResult};
use crate::config::Person;
use crate::extract::{Body, Caller};
use crate::kernel::{Kernel, HARNESS_VERSION};
use crate::{api_bodies as req, people::rights};
use axum::{
    extract::{ConnectInfo, Path, Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Extension, Json, Router,
};
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::Arc};

type K = State<Arc<Kernel>>;

pub(crate) use crate::api_error::{err, internal};
pub(crate) use sign_in::{issue, on_this_machine, over_tls};
pub(crate) use work::{chosen_model, end_for, status_str, task_row, terms, timeline_of, title_of};

/// New work is refused while the kill switch is on, before anything about
/// it is recorded (owner's ruling 11).
pub(crate) fn refused_by_kill_switch(k: &crate::kernel::Kernel) -> Option<Response> {
    k.read(|s| s.vault.kill_switch()).then(|| internal(crate::kernel::KillSwitchOn.into()))
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
    k.accounts.session(token).await.map_err(session_unread)?.and_then(|s| k.person(&s.person)).ok_or_else(|| err(StatusCode::UNAUTHORIZED, "sign in again"))
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
    // Those forms sign the browser in, so another site's page must not be
    // able to submit one and sign its visitor in to an account it chose.
    if recovery && !same_origin(h) {
        return err(StatusCode::FORBIDDEN, "this form is sent from its own page");
    }
    next.run(req).await
}

/// Whether a browser sent this request from one of the server's own pages:
/// `Sec-Fetch-Site` when the browser says, else an `Origin` naming this
/// host. A client that sends neither is no browser and is let through.
fn same_origin(h: &HeaderMap) -> bool {
    if let Some(site) = h.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        return matches!(site, "same-origin" | "none");
    }
    let Some(origin) = h.get("origin").and_then(|v| v.to_str().ok()) else { return true };
    let host = h.get("host").and_then(|v| v.to_str().ok());
    url::Url::parse(origin).ok().is_some_and(|o| {
        let authority = match (o.host_str(), o.port()) {
            (Some(name), Some(port)) => format!("{name}:{port}"),
            (Some(name), None) => name.to_string(),
            _ => return false,
        };
        host.is_some_and(|h| h.eq_ignore_ascii_case(&authority))
    })
}

pub fn router(k: Arc<Kernel>, transport: Transport) -> Router {
    use {owner::*, sign_in::*, work::*};
    Router::new()
        .route("/", get(|| crate::ui::assets::ui_file(Path("index.html".into()))))
        .route(crate::connectors::web::RETURN_PATH, get(crate::connectors::web::connected))
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
        .route("/v1/tasks/stop-all", post(stop_all))
        .route("/v1/chat", post(chat_post).get(chat_threads))
        .route("/v1/chat/{thread}", get(chat_thread))
        .route("/v1/chat/cards/{card}", post(chat_card))
        .route("/v1/tasks/{id}", get(task))
        .route("/v1/tasks/{id}/timeline", get(timeline))
        .route("/v1/tasks/{id}/resume", post(resume))
        .route("/v1/tasks/{id}/cancel", post(cancel))
        .route("/v1/tasks/{id}/steer", post(steer))
        .route("/v1/tasks/{id}/verdict", post(verdict))
        .route("/v1/search", get(search))
        .route("/v1/inbox", get(inbox_list))
        .route("/v1/inbox/{id}", post(respond))
        .route("/v1/inbox/opened", post(inbox_opened))
        .route("/v1/staged", get(crate::staging::list_route))
        .route("/v1/staged/{id}", post(crate::staging::resolve_route))
        .route("/v1/explore/scan", post(explore_scan))
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
        .route("/v1/memory/summary", get(memory_summary))
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
async fn require_device_cert(State(k): State<Arc<Kernel>>, req: Request, next: Next) -> ApiResult {
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
        return refuse(StatusCode::UNAUTHORIZED, msg);
    }
    Ok(next.run(req).await)
}

/// The config's long-lived tokens (the owner's scripts, the evaluation
/// runner) work only on the plain listener, from this machine: nowhere a
/// stolen copy could be used from the network (owner's ruling 13).
async fn config_tokens_on_this_machine(State(k): State<Arc<Kernel>>, req: Request, next: Next) -> ApiResult {
    if credential(req.headers()).is_some_and(|t| k.cfg.person_by_token(t).is_some()) {
        let plain = req.extensions().get::<Transport>() == Some(&Transport::Plain);
        let local = req.extensions().get::<axum::extract::ConnectInfo<SocketAddr>>().is_some_and(|c| c.0.ip().is_loopback());
        if !(plain && local) {
            return refuse(StatusCode::UNAUTHORIZED, "This key works only on the server itself, over its local address. Sign in to use Magnus from elsewhere.");
        }
    }
    Ok(next.run(req).await)
}

async fn me(State(k): K, h: HeaderMap) -> ApiResult {
    Ok(match who(&k, &h).await {
        Ok(p) => Json(json!({ "id": p.id, "name": p.name, "groups": p.groups, "roles": p.roles, "voice": k.cfg.speech.is_some(), "read_aloud": k.cfg.read_aloud.is_some() }))
            .into_response(),
        Err(r) => r,
    })
}

/// What the appliance is running now, and every change to it, so apps can
/// tell people when the agent they work with has changed (U5).
async fn versions(State(k): K, _: Caller) -> ApiResult {
    let v = k.read(|s| {
        let changes: Vec<Value> = s.org.version_changes().iter().map(|(seq, at, what, value)| json!({ "seq": seq, "at": at, "what": what, "value": value })).collect();
        json!({
            "harness": crate::kernel::HARNESS_VERSION,
            "instructions": crate::run::instructions_version(),
            "chat_instructions": crate::run::chat_instructions_version(),
            "model": s.org.model().as_ref().map(|(a, m)| format!("{a}/{m}")),
            "playbook": s.library.playbook_version(),
            "latest_seq": s.org.version_changes().last().map(|c| c.0),
            "changes": changes,
        })
    });
    Ok(Json(v).into_response())
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

async fn memory(State(k): K, Caller(p): Caller) -> ApiResult {
    // Recalled for the person, through their wall.
    let items: Vec<Value> = match k.remembered(&p.id, |_| true).await {
        Ok(r) => r
            .into_iter()
            .map(|r| {
                let m = r.entry;
                json!({ "id": m.id, "fact": r.fact, "topic": r.topic, "label": m.label, "run": m.run, "by": m.by.as_deref().unwrap_or("model"), "at": m.at })
            })
            .collect(),
        Err(e) => return Ok(internal(e)),
    };
    Ok(Json(json!({ "memory": items })).into_response())
}

async fn memory_summary(State(k): K, Caller(p): Caller) -> ApiResult {
    Ok(Json(k.memory_summary_json(&p.id)).into_response())
}

async fn explore_scan(State(k): K, Caller(p): Caller) -> ApiResult {
    match k.scan_for_ideas(&p.id).await {
        Ok(started) => Ok(Json(json!({ "started": started })).into_response()),
        Err(e) => Ok(err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))),
    }
}

/// Forget one of the person's own memories.
async fn forget(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    if !k.read(|s| s.library.memory().iter().any(|m| m.id == id && m.person == p.id)) {
        return refuse(StatusCode::NOT_FOUND, "no such memory");
    }
    Ok(crate::trash::trashed(k.trash(&p.id, rights(&p).moderates_shared_knowledge(), &id).await))
}

async fn health(State(k): K) -> ApiResult {
    // Down while the log's connection is lost: nothing can be recorded.
    let log = k.log.up().await;
    let body = Json(
        json!({ "ok": log, "log": if log { "up" } else { "down" }, "version": HARNESS_VERSION, "release": crate::update::version(), "kill_switch": k.read(|s| s.vault.kill_switch()) }),
    );
    Ok(if log { body.into_response() } else { (StatusCode::SERVICE_UNAVAILABLE, body).into_response() })
}

/// Speech to text on the appliance. The person reviews the text before sending it.
async fn transcribe(State(k): K, _: Caller, Body(b): Body<req::Transcribe>) -> ApiResult {
    let Some(data) = b.data_b64.as_deref() else { return refuse(StatusCode::BAD_REQUEST, "no audio") };
    Ok(match crate::speech::transcribe(k.cfg.speech.as_ref(), &k.cfg.appliance.state_dir, data).await {
        Ok(text) => Json(json!({ "text": text })).into_response(),
        Err(e) => err(StatusCode::SERVICE_UNAVAILABLE, format!("{e:#}")),
    })
}

/// Text to speech on the appliance, so every app and browser hears the same
/// voice. 503 when no voice is installed: the app then uses its system's own.
async fn speak(State(k): K, _: Caller, Body(b): Body<req::Speak>) -> ApiResult {
    let Some(cfg) = &k.cfg.read_aloud else { return refuse(StatusCode::SERVICE_UNAVAILABLE, "no read-aloud voice is installed on this server") };
    let Some(text) = b.text.as_deref().filter(|t| !t.trim().is_empty()) else { return refuse(StatusCode::BAD_REQUEST, "no text") };
    use crate::speech::SpeakError;
    Ok(match crate::speech::speak_limited(cfg, text).await {
        Ok(mp3) => Json(json!({ "media_type": "audio/mpeg", "audio_b64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, mp3) })).into_response(),
        Err(SpeakError::TooLong) => err(StatusCode::PAYLOAD_TOO_LARGE, format!("read aloud takes at most {} characters at a time", crate::speech::MAX_SPEAK)),
        Err(SpeakError::Busy) => err(StatusCode::TOO_MANY_REQUESTS, "the voice is busy; try again in a moment"),
        Err(SpeakError::TimedOut) => err(StatusCode::GATEWAY_TIMEOUT, "reading this aloud took too long"),
        Err(SpeakError::Failed(e)) => {
            tracing::warn!("read aloud: {e:#}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "read aloud failed")
        }
    })
}

#[cfg(test)]
mod same_origin_tests {
    use super::same_origin;
    use axum::http::HeaderMap;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, v.parse().unwrap());
        }
        h
    }

    /// Another site's page submitting the join form is refused; the
    /// server's own page, a typed address and a non-browser client are not.
    #[test]
    fn a_form_from_another_site_is_refused() {
        assert!(!same_origin(&headers(&[("sec-fetch-site", "cross-site")])));
        assert!(!same_origin(&headers(&[("sec-fetch-site", "same-site")])));
        assert!(!same_origin(&headers(&[("origin", "https://evil.example"), ("host", "magnus.example")])));
        assert!(same_origin(&headers(&[("sec-fetch-site", "same-origin")])));
        assert!(same_origin(&headers(&[("origin", "https://magnus.example:8443"), ("host", "magnus.example:8443")])));
        assert!(same_origin(&headers(&[])));
    }
}
