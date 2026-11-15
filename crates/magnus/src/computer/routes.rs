//! The computers' HTTP API.

use super::apps::NewApp;
use super::KillMode;
use crate::api::who;
use crate::api_error::ApiResult;
use crate::extract::Caller;
use crate::grants::answered;
use crate::kernel::Kernel;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

type K = State<Arc<Kernel>>;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .merge(crate::ui_actions::router())
        .merge(super::devview::router())
        .route("/v1/computers", get(list).post(create))
        .route("/v1/computers/{id}", get(one))
        .route("/v1/computers/{id}/start", post(start))
        .route("/v1/computers/{id}/wake", post(wake))
        .route("/v1/computers/{id}/kill", post(kill))
        .route("/v1/computers/{id}/reset", post(reset))
        .route("/v1/computers/{id}/grants", post(grant))
        .route("/v1/computers/{id}/files", get(read_file).put(write_file).delete(delete_file))
        .route("/v1/computers/{id}/listing", get(listing))
        .route("/v1/computers/{id}/files/raw", get(raw_file))
        .route("/v1/computers/{id}/export", get(super::export::export))
        .route("/v1/computers/{id}/screen", get(screen))
        .route("/v1/computers/{id}/destinations", post(add_destination))
        .route("/v1/computers/{id}/pages", post(open_page))
        .route("/v1/computers/{id}/pages/{page}", delete(close_page))
        .route("/v1/computers/{id}/hibernate", post(hibernate))
        .route("/v1/computers/{id}/theme", put(theme))
        .route("/v1/computers/{id}/backup", post(backup))
        .route("/v1/computers/{id}/restore", post(restore))
        .route("/v1/computers/{id}/apps", get(list_apps).post(install_app))
        .route("/v1/computers/{id}/apps/{name}", delete(remove_app))
        .route("/v1/computers/{id}/takeover", post(takeover))
        .route("/v1/computers/{id}/handback", post(handback))
        .route("/v1/computers/{id}/handback/closes", get(handback_closes))
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewIdentity {
    site: String,
}
async fn new_identity(State(k): K, Caller(p): Caller, Json(b): Json<NewIdentity>) -> ApiResult {
    Ok(answered(k.create_identity(&p, &b.site).await))
}
async fn identities(State(k): K, Caller(p): Caller) -> ApiResult {
    Ok(Json(k.identities_of(&p)).into_response())
}
async fn delete_identity(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(answered(k.delete_identity(&p, &id).await))
}
async fn secure_form(State(k): K, Caller(p): Caller, Path(id): Path<String>, Json(b): Json<super::identity::SecureForm>) -> ApiResult {
    Ok(answered(k.fill_secure_form(&p, &id, &b).await))
}
/// `Debug` is deliberately not derived: this holds the secret itself.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewSurrogate {
    name: String,
    value: String,
    host: String,
}
async fn save_attachment(State(k): K, Caller(p): Caller, Path((task, name)): Path<(String, String)>) -> ApiResult {
    Ok(answered(k.save_attachment(&p, &task, &name).await))
}
async fn new_surrogate(State(k): K, Caller(p): Caller, Path(id): Path<String>, Json(b): Json<NewSurrogate>) -> ApiResult {
    Ok(answered(k.issue_surrogate(&p, &id, &b.name, &b.value, &b.host).await))
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct NewComputer {
    #[serde(default)]
    workspace: Option<String>,
}

/// Nobody makes a computer (spec rev 142): a client from before that asks
/// for one is given the one there is.
async fn create(State(k): K, Caller(p): Caller, b: Option<Json<NewComputer>>) -> ApiResult {
    let b = b.map(|Json(b)| b).unwrap_or_default();
    Ok(answered(k.my_computer(&p, b.workspace.as_deref()).await))
}

async fn list(State(k): K, Caller(p): Caller) -> ApiResult {
    Ok(answered(Ok(k.computers_of(&p))))
}

async fn one(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(answered(k.computer_json(&p, &id)))
}

async fn start(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(answered(k.start_computer(&p, &id).await))
}

async fn wake(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(answered(k.wake_computer(&p, &id)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Kill {
    mode: KillMode,
    #[serde(default)]
    delete_volume: bool,
}

async fn kill(State(k): K, Caller(p): Caller, Path(id): Path<String>, Json(b): Json<Kill>) -> ApiResult {
    Ok(answered(k.kill_computer(&p, &id, b.mode, b.delete_volume).await))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reset {
    keep_files: bool,
}

async fn reset(State(k): K, Caller(p): Caller, Path(id): Path<String>, Json(b): Json<Reset>) -> ApiResult {
    Ok(answered(k.reset_computer(&p, &id, b.keep_files).await))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewGrant {
    label: String,
}

async fn grant(State(k): K, Caller(p): Caller, Path(id): Path<String>, Json(b): Json<NewGrant>) -> ApiResult {
    Ok(answered(k.grant_computer(&p, &id, &b.label).await))
}

#[derive(Deserialize)]
struct FilePath {
    path: String,
}

async fn listing(State(k): K, Caller(p): Caller, Path(id): Path<String>, Query(q): Query<FilePath>) -> ApiResult {
    Ok(answered(k.list_computer_files(&p, &id, &q.path).await))
}

/// The file itself, for the person to keep.
async fn raw_file(State(k): K, Caller(p): Caller, Path(id): Path<String>, Query(q): Query<FilePath>) -> ApiResult {
    use base64::Engine;
    let read = k.read_computer_file(&p, &id, &q.path).await;
    let bytes = read.map(|v: serde_json::Value| base64::engine::general_purpose::STANDARD.decode(v["data_b64"].as_str().unwrap_or_default()).unwrap_or_default());
    Ok(match bytes {
        Ok(b) => ([(axum::http::header::CONTENT_TYPE, "application/octet-stream")], b).into_response(),
        Err(e) => answered(Err(e)),
    })
}

async fn screen(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(answered(k.computer_preview(&p, &id).await))
}

async fn read_file(State(k): K, Caller(p): Caller, Path(id): Path<String>, Query(q): Query<FilePath>) -> ApiResult {
    Ok(answered(k.read_computer_file(&p, &id, &q.path).await))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteFile {
    path: String,
    data_b64: String,
}

async fn write_file(State(k): K, Caller(p): Caller, Path(id): Path<String>, Json(b): Json<WriteFile>) -> ApiResult {
    Ok(answered(k.write_computer_file(&p, &id, &b.path, &b.data_b64).await))
}

async fn delete_file(State(k): K, Caller(p): Caller, Path(id): Path<String>, Query(q): Query<FilePath>) -> ApiResult {
    Ok(answered(k.delete_computer_file(&p, &id, &q.path).await))
}

async fn hibernate(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(answered(k.hibernate_computer(&p, &id).await))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesktopTheme {
    theme: super::Theme,
}

/// The person's app theme, for their computer's desktop.
async fn theme(State(k): K, Caller(p): Caller, Path(id): Path<String>, Json(b): Json<DesktopTheme>) -> ApiResult {
    Ok(answered(k.set_desktop_theme(&p, &id, b.theme).await))
}

async fn backup(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(answered(k.backup_computer(&p, &id).await))
}

async fn restore(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(answered(k.restore_computer(&p, &id).await))
}

async fn list_apps(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(answered(k.computer_apps(&p, &id)))
}

async fn install_app(State(k): K, Caller(p): Caller, Path(id): Path<String>, Json(b): Json<NewApp>) -> ApiResult {
    Ok(answered(k.record_computer_app(&p, &id, b).await))
}

async fn remove_app(State(k): K, Caller(p): Caller, Path((id, name)): Path<(String, String)>) -> ApiResult {
    Ok(answered(k.remove_computer_app(&p, &id, &name).await))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewDestination {
    host: String,
}

async fn add_destination(State(k): K, Caller(p): Caller, Path(id): Path<String>, Json(b): Json<NewDestination>) -> ApiResult {
    Ok(answered(k.add_computer_destination(&p, &id, &b.host).await))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewPage {
    url: String,
}

async fn open_page(State(k): K, Caller(p): Caller, Path(id): Path<String>, Json(b): Json<NewPage>) -> ApiResult {
    Ok(answered(k.open_computer_page(&p, &id, &b.url).await))
}

async fn close_page(State(k): K, Caller(p): Caller, Path((id, page)): Path<(String, String)>) -> ApiResult {
    Ok(answered(k.close_computer_page(&p, &id, &page).await))
}

async fn takeover(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(answered(k.start_takeover(&p, &id).await))
}

async fn handback(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(answered(k.hand_back(&p, &id).await))
}

/// What "I'm done" would close, asked before it is done.
async fn handback_closes(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(answered(k.handback_closes(&p, &id).await))
}

async fn activity(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(answered(k.computer_activity(&p, &id)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewOperator {
    operator: String,
    minutes: i64,
}

async fn grant_operator(State(k): K, Caller(p): Caller, Path(id): Path<String>, Json(b): Json<NewOperator>) -> ApiResult {
    Ok(answered(k.grant_operator(&p, &id, &b.operator, b.minutes).await))
}

async fn end_operator(State(k): K, Caller(p): Caller, Path((id, session)): Path<(String, String)>) -> ApiResult {
    Ok(answered(k.end_operator(&p, &id, &session).await))
}

async fn operator_file(State(k): K, Caller(p): Caller, Path(id): Path<String>, Query(q): Query<FilePath>) -> ApiResult {
    Ok(answered(k.operator_file(&p, &id, &q.path).await))
}

async fn operator_screen(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(answered(k.operator_screen(&p, &id).await))
}

/// The ticket comes as a WebSocket subprotocol, so it is never in a URL and
/// no sign-in header is needed from a browser. A signed-in caller must be
/// the person it was issued to.
async fn display(State(k): K, h: HeaderMap, Path(id): Path<String>, ws: WebSocketUpgrade) -> ApiResult {
    let offered = h.get("sec-websocket-protocol").and_then(|v| v.to_str().ok()).unwrap_or_default();
    let ticket = offered.split(',').map(str::trim).find_map(|p| p.strip_prefix(super::takeover::TICKET_PROTOCOL_PREFIX));
    let Some(person) = ticket.and_then(|t| super::takeover::redeem(t, &id)) else {
        return Ok(crate::api::err(StatusCode::FORBIDDEN, "that ticket is not valid"));
    };
    if h.contains_key("authorization") && who(&k, &h).await.map(|p| p.id != person).unwrap_or(true) {
        return Ok(crate::api::err(StatusCode::FORBIDDEN, "that ticket is not yours"));
    }
    let deflate = super::display::wants_deflate(offered);
    let cut = super::display::cut_signal(&id);
    Ok(match k.display_stream(&id, &person).await {
        Ok(stream) => ws.protocols(super::display::PROTOCOLS).on_upgrade(move |socket| super::display::carry(socket, stream, deflate, id, cut)),
        Err(e) => answered(Err(e)),
    })
}
