//! The workspaces' HTTP API.
use super::agent::{held_dir, volume_dir};
use super::{fit_name, Change, File, Message, Role};
use crate::api::{err, who};
use crate::config::Person;
use crate::events::Ev;
use crate::kernel::Kernel;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

const MAX_UPLOAD_BYTES: usize = 32 * 1024 * 1024;
const MAX_MESSAGE_CHARS: usize = 20_000;
const MAX_NAME_CHARS: usize = 80;

pub(super) type K = State<Arc<Kernel>>;
pub(super) type Reply = Response;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/workspaces", get(list).post(create))
        .route("/v1/workspaces/{ws}", get(one).delete(remove))
        .route("/v1/workspaces/{ws}/members", post(set_member))
        .route("/v1/workspaces/{ws}/members/{person}", delete(remove_member))
        .route("/v1/workspaces/{ws}/channels", post(add_channel))
        .route("/v1/workspaces/{ws}/channels/{channel}/messages", get(messages).post(post_message))
        .route("/v1/workspaces/{ws}/connectors", post(grant_connector))
        .route("/v1/workspaces/{ws}/files/{name}", get(read_file).put(write_file).delete(delete_file).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)))
        .route("/v1/workspaces/{ws}/held/{name}/share", post(share_held))
        .merge(super::kb_routes::router())
        .merge(super::whisper::router())
        .merge(super::routines::router())
        .merge(super::upkeep::router())
}

pub(super) fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub(super) async fn send(k: &Kernel, p: &Person, c: Change) -> Result<(), Box<Reply>> {
    k.emit(&format!("person:{}", p.id), Ev::Team(c)).await.map(|_| ()).map_err(|e| Box::new(err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))))
}

/// The caller and their role here, or a 404 that does not say the workspace exists.
pub(super) async fn member(k: &Kernel, h: &HeaderMap, ws: &str, at_least: Role) -> Result<(Person, Role), Box<Reply>> {
    let p = who(k, h).await.map_err(Box::new)?;
    let Some(role) = k.read(|s| s.team.role(ws, &p.id)) else { return Err(Box::new(err(StatusCode::NOT_FOUND, "no such workspace"))) };
    if role < at_least {
        return Err(Box::new(err(StatusCode::FORBIDDEN, format!("that takes the {at_least:?} role here"))));
    }
    Ok((p, role))
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

fn short(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= MAX_NAME_CHARS
}

async fn list(State(k): K, h: HeaderMap) -> Reply {
    let p = ok_or_return!(who(&k, &h).await.map_err(Box::new));
    let v: Vec<Value> = k.read(|s| s.team.of(&p.id).into_iter().map(|w| json!({ "id": w.id, "name": w.name, "role": w.members.get(&p.id) })).collect());
    Json(v).into_response()
}

async fn create(State(k): K, h: HeaderMap, Json(b): Json<Named>) -> Reply {
    let p = ok_or_return!(who(&k, &h).await.map_err(Box::new));
    if !(p.is_owner() || p.has_role("admin")) {
        return err(StatusCode::FORBIDDEN, "an owner or admin creates a workspace");
    }
    if !short(&b.name) {
        return err(StatusCode::BAD_REQUEST, "a workspace needs a short name");
    }
    let id = new_id();
    ok_or_return!(send(&k, &p, Change::Created { workspace: id.clone(), name: b.name, owner: p.id.clone() }).await);
    Json(json!({ "id": id })).into_response()
}

async fn one(State(k): K, h: HeaderMap, Path(ws): Path<String>) -> Reply {
    let (p, role) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    let w = k.read(|s| s.team.workspace(&ws).cloned());
    let Some(w) = w else { return err(StatusCode::NOT_FOUND, "no such workspace") };
    let channels: Vec<Value> = w.channels.iter().map(|(id, c)| json!({ "id": id, "name": c.name })).collect();
    Json(json!({ "id": w.id, "name": w.name, "role": role, "members": w.members, "channels": channels, "connectors": w.connectors, "files": w.files.values().collect::<Vec<_>>(), "knowledge": w.entries(None).iter().map(|e| (e.id.clone(), e.statement.clone())).collect::<BTreeMap<_, _>>(), "local_ceiling": w.local_ceiling, "approvers": w.approvers, "routines": w.routines, "projects": w.projects.values().filter(|d| d.may_read(&p.id)).map(|d| json!({ "id": d.id, "name": d.name })).collect::<Vec<_>>() })).into_response()
}

async fn remove(State(k): K, h: HeaderMap, Path(ws): Path<String>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Owner).await);
    if let Err(e) = k.delete_workspace(&p, &ws).await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"));
    }
    Json(json!({ "ok": true })).into_response()
}

#[derive(Deserialize)]
struct SetMember {
    person: String,
    role: Role,
}

async fn set_member(State(k): K, h: HeaderMap, Path(ws): Path<String>, Json(b): Json<SetMember>) -> Reply {
    let (p, role) = ok_or_return!(member(&k, &h, &ws, Role::Admin).await);
    if b.role > role {
        return err(StatusCode::FORBIDDEN, "you cannot give a role above your own");
    }
    if k.person(&b.person).is_none() {
        return err(StatusCode::NOT_FOUND, "no such person");
    }
    ok_or_return!(send(&k, &p, Change::Member { workspace: ws, person: b.person, role: b.role }).await);
    Json(json!({ "ok": true })).into_response()
}

async fn remove_member(State(k): K, h: HeaderMap, Path((ws, person)): Path<(String, String)>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Admin).await);
    let running: Vec<String> = k.read(|s| s.team.runs_by(&ws, &person).into_iter().map(|r| r.task.clone()).collect());
    ok_or_return!(send(&k, &p, Change::Removed { workspace: ws.clone(), person: person.clone() }).await);
    k.end_team_tasks(&format!("person:{}", p.id), &running).await;
    k.remove_held(&ws, &person).await;
    Json(json!({ "ok": true })).into_response()
}

async fn add_channel(State(k): K, h: HeaderMap, Path(ws): Path<String>, Json(b): Json<Named>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    if !short(&b.name) {
        return err(StatusCode::BAD_REQUEST, "a channel needs a short name");
    }
    let id = new_id();
    ok_or_return!(send(&k, &p, Change::Channel { workspace: ws, channel: id.clone(), name: b.name }).await);
    Json(json!({ "id": id })).into_response()
}

fn has_channel(k: &Kernel, ws: &str, channel: &str) -> bool {
    k.read(|s| s.team.workspace(ws).is_some_and(|w| w.channels.contains_key(channel)))
}

/// A channel's messages, with each finished request's answer in its place.
async fn messages(State(k): K, h: HeaderMap, Path((ws, channel)): Path<(String, String)>) -> Reply {
    ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    if !has_channel(&k, &ws, &channel) {
        return err(StatusCode::NOT_FOUND, "no such channel");
    }
    let view = k.read(|s| {
        let mut out: Vec<Value> = s.team.workspace(&ws).and_then(|w| w.channels.get(&channel)).map(|c| c.messages.iter().map(|m| json!(m)).collect()).unwrap_or_default();
        for r in s.team.runs_in(&ws, &channel) {
            let answer = s.tasks.get(&r.task).and_then(|t| s.runs.get(&t.run_id)).filter(|run| run.status.terminal()).and_then(|run| run.summary.clone());
            if let Some(text) = answer {
                out.push(json!({ "sender": "agent", "requester": r.requester, "task": r.task, "text": text, "thread": r.thread }));
            }
        }
        out
    });
    Json(view).into_response()
}

#[derive(Deserialize)]
struct NewMessage {
    text: String,
    #[serde(default)]
    thread: Option<String>,
    /// Asks the team agent to act on this message.
    #[serde(default)]
    agent: bool,
    /// For the sender's own agent only, and it does not reach the channel.
    #[serde(default)]
    whisper: bool,
}

async fn post_message(State(k): K, h: HeaderMap, Path((ws, channel)): Path<(String, String)>, Json(b): Json<NewMessage>) -> Reply {
    let (p, role) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    if !has_channel(&k, &ws, &channel) {
        return err(StatusCode::NOT_FOUND, "no such channel");
    }
    if b.text.trim().is_empty() || b.text.chars().count() > MAX_MESSAGE_CHARS {
        return err(StatusCode::BAD_REQUEST, "a message needs text, and not too much of it");
    }
    if (b.agent || b.whisper) && role == Role::Guest {
        return err(StatusCode::FORBIDDEN, "a guest can read and chat, and cannot have the agent act");
    }
    if b.whisper {
        return match k.whisper(&ws, &channel, b.thread, &p.id, b.text).await {
            Ok((task, run)) => Json(json!({ "task_id": task, "run_id": run })).into_response(),
            Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
        };
    }
    let message = Message { id: new_id(), sender: p.id.clone(), text: b.text.clone(), thread: b.thread.clone(), at: chrono::Utc::now() };
    let id = message.id.clone();
    ok_or_return!(send(&k, &p, Change::Message { workspace: ws.clone(), channel: channel.clone(), message }).await);
    if !b.agent {
        return Json(json!({ "id": id })).into_response();
    }
    match k.team_request(&ws, &channel, b.thread, &p.id, b.text).await {
        Ok((task, run)) => Json(json!({ "id": id, "task_id": task, "run_id": run })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}

#[derive(Deserialize)]
struct GrantConnector {
    app: String,
    roles: Vec<Role>,
}

async fn grant_connector(State(k): K, h: HeaderMap, Path(ws): Path<String>, Json(b): Json<GrantConnector>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Admin).await);
    if !k.connectors().iter().any(|c| c.name == b.app) {
        return err(StatusCode::NOT_FOUND, "no such app");
    }
    ok_or_return!(send(&k, &p, Change::Connector { workspace: ws, app: b.app, roles: b.roles }).await);
    Json(json!({ "ok": true })).into_response()
}

async fn read_file(State(k): K, h: HeaderMap, Path((ws, name)): Path<(String, String)>) -> Reply {
    ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    let known = k.read(|s| s.team.workspace(&ws).is_some_and(|w| w.files.contains_key(&name)));
    if !known || !fit_name(&name) {
        return err(StatusCode::NOT_FOUND, "no such file");
    }
    match tokio::fs::read(volume_dir(&k, &ws).join(&name)).await {
        Ok(bytes) => bytes.into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")),
    }
}

/// An explicit share: the only way a member's file enters the volume.
async fn write_file(State(k): K, h: HeaderMap, Path((ws, name)): Path<(String, String)>, _q: Query<HashMap<String, String>>, body: Bytes) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    if !fit_name(&name) {
        return err(StatusCode::BAD_REQUEST, "that is not a file name");
    }
    let dir = volume_dir(&k, &ws);
    if let Err(e) = async {
        tokio::fs::create_dir_all(&dir).await?;
        tokio::fs::write(dir.join(&name), &body).await
    }
    .await
    {
        return err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}"));
    }
    let file = File { name, size: body.len() as u64, by: p.id.clone() };
    ok_or_return!(send(&k, &p, Change::File { workspace: ws, file }).await);
    Json(json!({ "ok": true })).into_response()
}

/// Whoever put it there, or an owner or admin, may delete a file.
async fn delete_file(State(k): K, h: HeaderMap, Path((ws, name)): Path<(String, String)>) -> Reply {
    let (p, role) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    let by = k.read(|s| s.team.workspace(&ws).and_then(|w| w.files.get(&name).map(|f| f.by.clone())));
    let Some(by) = by else { return err(StatusCode::NOT_FOUND, "no such file") };
    if by != p.id && role < Role::Admin {
        return err(StatusCode::FORBIDDEN, "only who added it, or an admin, may delete it");
    }
    if fit_name(&name) {
        if let Err(e) = tokio::fs::remove_file(volume_dir(&k, &ws).join(&name)).await {
            tracing::warn!("{name}: {e}");
        }
    }
    ok_or_return!(send(&k, &p, Change::FileRemoved { workspace: ws, name }).await);
    Json(json!({ "ok": true })).into_response()
}

/// The requester's own choice to put a held file on the team volume.
async fn share_held(State(k): K, h: HeaderMap, Path((ws, name)): Path<(String, String)>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    let held = k.read(|s| s.team.workspace(&ws).and_then(|w| w.held.get(&p.id).and_then(|f| f.get(&name).cloned())));
    let Some(file) = held else { return err(StatusCode::NOT_FOUND, "you hold no such file") };
    let target = volume_dir(&k, &ws);
    if let Err(e) = async {
        tokio::fs::create_dir_all(&target).await?;
        tokio::fs::rename(held_dir(&k, &ws, &p.id).join(&name), target.join(&name)).await
    }
    .await
    {
        return err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}"));
    }
    ok_or_return!(send(&k, &p, Change::File { workspace: ws.clone(), file }).await);
    ok_or_return!(send(&k, &p, Change::Released { workspace: ws, person: p.id.clone(), name }).await);
    Json(json!({ "ok": true })).into_response()
}
