//! The desktop app's side of the bridge, over its own session: it reports
//! its computer's programs, takes calls on a long poll and answers them;
//! the person allows or takes back a program on a computer.
use crate::api::{credential, err, who};
use crate::config::Person;
use crate::desktop::{self, Change, Reported};
use crate::kernel::Kernel;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// How long one poll waits for a call before the app polls again.
const NEXT_WAIT: Duration = Duration::from_secs(20);
const MAX_NAME_CHARS: usize = 80;
const MIN_ID_CHARS: usize = 8;
const MAX_ID_CHARS: usize = 64;
const MAX_REPORTED: usize = 16;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/desktop/devices", get(list).post(announce))
        .route("/v1/desktop/devices/{id}", delete(remove))
        .route("/v1/desktop/devices/{id}/programs/{program}", put(consent))
        .route("/v1/desktop/devices/{id}/next", get(next))
        .route("/v1/desktop/devices/{id}/result", post(result))
}

type K = State<Arc<Kernel>>;

/// The bridge takes calls only on the app's own session: a bearer token the
/// app signed in for, never a browser's cookie.
pub(crate) async fn app_session(k: &Kernel, h: &HeaderMap) -> Result<Person, Response> {
    let p = who(k, h).await?;
    let token = credential(h).unwrap_or_default();
    let bearer = h.get("authorization").and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("Bearer "));
    match k.accounts.session(token).await {
        Some(s) if bearer && s.client == "app" => Ok(p),
        _ => Err(err(StatusCode::FORBIDDEN, "only the Magnus app on that computer does this")),
    }
}

pub(crate) fn valid_id(id: &str) -> bool {
    (MIN_ID_CHARS..=MAX_ID_CHARS).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn mine(k: &Kernel, person: &str, id: &str) -> Option<desktop::Device> {
    k.read(|s| s.local.desktop.get(id).filter(|d| d.person == person).cloned())
}

async fn list(State(k): K, h: HeaderMap) -> Response {
    match who(&k, &h).await {
        Ok(p) => Json(k.desktop_views(&p.id)).into_response(),
        Err(r) => r,
    }
}

#[derive(Deserialize)]
struct Announce {
    id: String,
    name: String,
    #[serde(default)]
    programs: Vec<Reported>,
}

/// The app's report: its computer, by the id it made and its name, and
/// what it found there. The answer is that computer as the server sees it,
/// with the programs the person allowed on it.
async fn announce(State(k): K, h: HeaderMap, Json(b): Json<Announce>) -> Response {
    let p = match app_session(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let name = b.name.trim().to_string();
    if !valid_id(&b.id) || name.is_empty() || name.chars().count() > MAX_NAME_CHARS || b.programs.len() > MAX_REPORTED {
        return err(StatusCode::BAD_REQUEST, "a computer needs an id, a short name and a short list of programs");
    }
    match k.read(|s| s.local.desktop.get(&b.id).cloned()) {
        Some(d) if d.person != p.id => return err(StatusCode::CONFLICT, "that computer id is another person's"),
        Some(d) if d.name == name => {}
        _ => {
            if let Err(e) = k.emit(&format!("person:{}", p.id), crate::events::Ev::Desktop(Change::Device { id: b.id.clone(), person: p.id.clone(), name })).await {
                return err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"));
            }
        }
    }
    desktop::report(&b.id, b.programs);
    match k.desktop_views(&p.id).into_iter().find(|v| v.id == b.id) {
        Some(v) => Json(v).into_response(),
        None => err(StatusCode::INTERNAL_SERVER_ERROR, "the computer was not kept"),
    }
}

async fn remove(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if mine(&k, &p.id, &id).is_none() {
        return err(StatusCode::NOT_FOUND, "no such computer");
    }
    match k.emit(&format!("person:{}", p.id), crate::events::Ev::Desktop(Change::Removed { id, person: p.id.clone() })).await {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}

#[derive(Deserialize)]
struct Consent {
    allowed: bool,
}

/// "Use <program> on this computer". Allowing is done from the app on that
/// computer; taking it back, from anywhere the person is signed in.
async fn consent(State(k): K, h: HeaderMap, Path((id, program)): Path<(String, String)>, Json(b): Json<Consent>) -> Response {
    let p = if b.allowed { app_session(&k, &h).await } else { who(&k, &h).await };
    let p = match p {
        Ok(p) => p,
        Err(r) => return r,
    };
    if desktop::program(&program).is_none() {
        return err(StatusCode::NOT_FOUND, "not a supported program");
    }
    if mine(&k, &p.id, &id).is_none() {
        return err(StatusCode::NOT_FOUND, "no such computer");
    }
    let ev = crate::events::Ev::Desktop(Change::Consent { device: id.clone(), person: p.id.clone(), program, allowed: b.allowed });
    if let Err(e) = k.emit(&format!("person:{}", p.id), ev).await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"));
    }
    match k.desktop_views(&p.id).into_iter().find(|v| v.id == id) {
        Some(v) => Json(v).into_response(),
        None => err(StatusCode::NOT_FOUND, "no such computer"),
    }
}

async fn next(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match app_session(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if mine(&k, &p.id, &id).is_none() {
        return err(StatusCode::NOT_FOUND, "no such computer");
    }
    let _polling = desktop::Polling::start(&id);
    match desktop::next_job(&id, NEXT_WAIT).await {
        Some(job) => Json(job).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

#[derive(Deserialize)]
struct Done {
    job: String,
    ok: bool,
    text: String,
}

async fn result(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Done>) -> Response {
    let p = match app_session(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if mine(&k, &p.id, &id).is_none() {
        return err(StatusCode::NOT_FOUND, "no such computer");
    }
    // The run may have stopped waiting; then there is no one to tell.
    let told = desktop::answer(&id, &b.job, b.ok, b.text);
    Json(json!({ "ok": true, "told": told })).into_response()
}
