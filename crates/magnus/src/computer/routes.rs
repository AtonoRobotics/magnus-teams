//! The computers' HTTP API.

use super::apps::NewApp;
use super::KillMode;
use crate::api::who;
use crate::grants::answered;
use crate::kernel::Kernel;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

const DISPLAY_BUFFER_BYTES: usize = 64 * 1024;

type K = State<Arc<Kernel>>;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .merge(crate::ui_actions::router())
        .route("/v1/computers", get(list).post(create))
        .route("/v1/computers/{id}", get(one))
        .route("/v1/computers/{id}/start", post(start))
        .route("/v1/computers/{id}/kill", post(kill))
        .route("/v1/computers/{id}/grants", post(grant))
        .route("/v1/computers/{id}/files", get(read_file).put(write_file).delete(delete_file))
        .route("/v1/computers/{id}/listing", get(listing))
        .route("/v1/computers/{id}/files/raw", get(raw_file))
        .route("/v1/computers/{id}/screen", get(screen))
        .route("/v1/computers/{id}/destinations", post(add_destination))
        .route("/v1/computers/{id}/pages", post(open_page))
        .route("/v1/computers/{id}/pages/{page}", delete(close_page))
        .route("/v1/computers/{id}/hibernate", post(hibernate))
        .route("/v1/computers/{id}/backup", post(backup))
        .route("/v1/computers/{id}/restore", post(restore))
        .route("/v1/computers/{id}/apps", get(list_apps).post(install_app))
        .route("/v1/computers/{id}/apps/{name}", delete(remove_app))
        .route("/v1/computers/{id}/takeover", post(takeover))
        .route("/v1/computers/{id}/handback", post(handback))
        .route("/v1/computers/{id}/display", get(display))
        .route("/v1/computers/{id}/activity", get(activity))
        .route("/v1/computers/{id}/operator-sessions", post(grant_operator))
        .route("/v1/computers/{id}/operator-sessions/{session}", delete(end_operator))
        .route("/v1/identities", get(identities).post(new_identity))
        .route("/v1/identities/{id}", delete(delete_identity))
        .route("/v1/identities/{id}/secure-form", post(secure_form))
        .route("/v1/computers/{id}/surrogates", post(new_surrogate))
        .route("/v1/tasks/{task}/attachments/{name}/save", post(save_attachment))
        .route("/v1/operator/computers/{id}/files", get(operator_file))
        .route("/v1/operator/computers/{id}/screen", get(operator_screen))
}

macro_rules! person {
    ($k:expr, $h:expr) => {
        match who(&$k, &$h).await {
            Ok(p) => p,
            Err(r) => return r,
        }
    };
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewIdentity {
    site: String,
}
async fn new_identity(State(k): K, h: HeaderMap, Json(b): Json<NewIdentity>) -> Response {
    let p = person!(k, h);
    answered(k.create_identity(&p, &b.site).await)
}
async fn identities(State(k): K, h: HeaderMap) -> Response {
    let p = person!(k, h);
    Json(k.identities_of(&p)).into_response()
}
async fn delete_identity(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = person!(k, h);
    answered(k.delete_identity(&p, &id).await)
}
async fn secure_form(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<super::identity::SecureForm>) -> Response {
    let p = person!(k, h);
    answered(k.fill_secure_form(&p, &id, &b).await)
}
/// `Debug` is deliberately not derived: this holds the secret itself.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewSurrogate {
    name: String,
    value: String,
    host: String,
}
async fn save_attachment(State(k): K, h: HeaderMap, Path((task, name)): Path<(String, String)>) -> Response {
    let p = person!(k, h);
    answered(k.save_attachment(&p, &task, &name).await)
}
async fn new_surrogate(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<NewSurrogate>) -> Response {
    let p = person!(k, h);
    answered(k.issue_surrogate(&p, &id, &b.name, &b.value, &b.host).await)
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct NewComputer {
    #[serde(default)]
    workspace: Option<String>,
}

async fn create(State(k): K, h: HeaderMap, b: Option<Json<NewComputer>>) -> Response {
    let p = person!(k, h);
    let b = b.map(|Json(b)| b).unwrap_or_default();
    answered(k.create_computer(&p, b.workspace.as_deref()).await)
}

async fn list(State(k): K, h: HeaderMap) -> Response {
    let p = person!(k, h);
    answered(Ok(k.computers_of(&p)))
}

async fn one(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = person!(k, h);
    answered(k.computer_json(&p, &id))
}

async fn start(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = person!(k, h);
    answered(k.start_computer(&p, &id).await)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Kill {
    mode: KillMode,
    #[serde(default)]
    delete_volume: bool,
}

async fn kill(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Kill>) -> Response {
    let p = person!(k, h);
    answered(k.kill_computer(&p, &id, b.mode, b.delete_volume).await)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewGrant {
    label: String,
}

async fn grant(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<NewGrant>) -> Response {
    let p = person!(k, h);
    answered(k.grant_computer(&p, &id, &b.label).await)
}

#[derive(Deserialize)]
struct FilePath {
    path: String,
}

async fn listing(State(k): K, h: HeaderMap, Path(id): Path<String>, Query(q): Query<FilePath>) -> Response {
    let p = person!(k, h);
    answered(k.list_computer_files(&p, &id, &q.path).await)
}

/// The file itself, for the person to keep.
async fn raw_file(State(k): K, h: HeaderMap, Path(id): Path<String>, Query(q): Query<FilePath>) -> Response {
    use base64::Engine;
    let p = person!(k, h);
    let read = k.read_computer_file(&p, &id, &q.path).await;
    let bytes = read.map(|v: serde_json::Value| base64::engine::general_purpose::STANDARD.decode(v["data_b64"].as_str().unwrap_or_default()).unwrap_or_default());
    match bytes {
        Ok(b) => ([(axum::http::header::CONTENT_TYPE, "application/octet-stream")], b).into_response(),
        Err(e) => answered(Err(e)),
    }
}

async fn screen(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = person!(k, h);
    answered(k.computer_preview(&p, &id).await)
}

async fn read_file(State(k): K, h: HeaderMap, Path(id): Path<String>, Query(q): Query<FilePath>) -> Response {
    let p = person!(k, h);
    answered(k.read_computer_file(&p, &id, &q.path).await)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteFile {
    path: String,
    data_b64: String,
}

async fn write_file(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<WriteFile>) -> Response {
    let p = person!(k, h);
    answered(k.write_computer_file(&p, &id, &b.path, &b.data_b64).await)
}

async fn delete_file(State(k): K, h: HeaderMap, Path(id): Path<String>, Query(q): Query<FilePath>) -> Response {
    let p = person!(k, h);
    answered(k.delete_computer_file(&p, &id, &q.path).await)
}

async fn hibernate(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = person!(k, h);
    answered(k.hibernate_computer(&p, &id).await)
}

async fn backup(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = person!(k, h);
    answered(k.backup_computer(&p, &id).await)
}

async fn restore(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = person!(k, h);
    answered(k.restore_computer(&p, &id).await)
}

async fn list_apps(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = person!(k, h);
    answered(k.computer_apps(&p, &id))
}

async fn install_app(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<NewApp>) -> Response {
    let p = person!(k, h);
    answered(k.record_computer_app(&p, &id, b).await)
}

async fn remove_app(State(k): K, h: HeaderMap, Path((id, name)): Path<(String, String)>) -> Response {
    let p = person!(k, h);
    answered(k.remove_computer_app(&p, &id, &name).await)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewDestination {
    host: String,
}

async fn add_destination(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<NewDestination>) -> Response {
    let p = person!(k, h);
    answered(k.add_computer_destination(&p, &id, &b.host).await)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewPage {
    url: String,
}

async fn open_page(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<NewPage>) -> Response {
    let p = person!(k, h);
    answered(k.open_computer_page(&p, &id, &b.url).await)
}

async fn close_page(State(k): K, h: HeaderMap, Path((id, page)): Path<(String, String)>) -> Response {
    let p = person!(k, h);
    answered(k.close_computer_page(&p, &id, &page).await)
}

async fn takeover(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = person!(k, h);
    answered(k.start_takeover(&p, &id).await)
}

async fn handback(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = person!(k, h);
    answered(k.hand_back(&p, &id).await)
}

async fn activity(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = person!(k, h);
    answered(k.computer_activity(&p, &id))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewOperator {
    operator: String,
    minutes: i64,
}

async fn grant_operator(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<NewOperator>) -> Response {
    let p = person!(k, h);
    answered(k.grant_operator(&p, &id, &b.operator, b.minutes).await)
}

async fn end_operator(State(k): K, h: HeaderMap, Path((id, session)): Path<(String, String)>) -> Response {
    let p = person!(k, h);
    answered(k.end_operator(&p, &id, &session).await)
}

async fn operator_file(State(k): K, h: HeaderMap, Path(id): Path<String>, Query(q): Query<FilePath>) -> Response {
    let p = person!(k, h);
    answered(k.operator_file(&p, &id, &q.path).await)
}

async fn operator_screen(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = person!(k, h);
    answered(k.operator_screen(&p, &id).await)
}

/// The ticket comes as a WebSocket subprotocol, so it is never in a URL and
/// no sign-in header is needed from a browser. A signed-in caller must be
/// the person it was issued to.
async fn display(State(k): K, h: HeaderMap, Path(id): Path<String>, ws: WebSocketUpgrade) -> Response {
    let offered = h.get("sec-websocket-protocol").and_then(|v| v.to_str().ok()).unwrap_or_default();
    let ticket = offered.split(',').map(str::trim).find_map(|p| p.strip_prefix(super::takeover::TICKET_PROTOCOL_PREFIX));
    let Some(person) = ticket.and_then(|t| super::takeover::redeem(t, &id)) else {
        return crate::api::err(StatusCode::FORBIDDEN, "that ticket is not valid");
    };
    if h.contains_key("authorization") && who(&k, &h).await.map(|p| p.id != person).unwrap_or(true) {
        return crate::api::err(StatusCode::FORBIDDEN, "that ticket is not yours");
    }
    match k.display_stream(&id, &person).await {
        Ok(stream) => ws.protocols([super::takeover::DISPLAY_PROTOCOL]).on_upgrade(move |socket| carry(socket, stream)),
        Err(e) => answered(Err(e)),
    }
}

/// Bytes both ways until either side stops.
async fn carry(mut socket: WebSocket, mut stream: UnixStream) {
    let mut buf = vec![0u8; DISPLAY_BUFFER_BYTES];
    loop {
        tokio::select! {
            from_screen = stream.read(&mut buf) => match from_screen {
                Ok(n) if n > 0 => {
                    if socket.send(Message::Binary(buf[..n].to_vec().into())).await.is_err() {
                        return;
                    }
                }
                _ => return,
            },
            from_person = socket.recv() => match from_person {
                Some(Ok(Message::Binary(b))) => {
                    if stream.write_all(&b).await.is_err() {
                        return;
                    }
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Text(_))) => {}
                _ => return,
            },
        }
    }
}
