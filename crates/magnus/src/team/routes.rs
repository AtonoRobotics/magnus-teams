//! The workspaces' HTTP API.
use super::agent::{held_dir, volume_dir};
use super::caller::{at, Member};
use super::chat::Chat;
use super::conversation::Why;
use super::may::{allow, Act};
use super::member_view::MemberView;
use super::{fit_name, Change, File, Message, Role};
use crate::api::{err, who};
use crate::api_error::{refuse, ApiResult};
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
/// The channel every new workspace starts with.
const FIRST_CHANNEL: &str = "general";

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
        .merge(super::chat_routes::router())
        .merge(super::project_routes::router())
        .merge(super::spend::router())
        .merge(super::search::router())
        .merge(super::whisper::router())
        .merge(super::routines::router())
        .merge(super::upkeep::router())
        .merge(super::own_view::router())
        .merge(super::membership::router())
        .merge(super::notices::router())
        .merge(super::message_actions::router())
        .merge(super::mission_routes::router())
        .merge(super::sign_in::router())
}

pub(super) fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub(super) async fn send(k: &Kernel, p: &Person, c: Change) -> Result<(), Box<Reply>> {
    k.emit(&format!("person:{}", p.id), Ev::Team(c)).await.map(|_| ()).map_err(|e| Box::new(err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))))
}

/// The caller and their role here, or a 404 that does not say the workspace exists.
pub(super) async fn member(k: &Kernel, h: &HeaderMap, ws: &str, at_least: Role) -> Result<(Person, Role), Box<Reply>> {
    let p = who(k, h).await?;
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

async fn list(State(k): K, h: HeaderMap) -> ApiResult {
    let p = who(&k, &h).await?;
    let v: Vec<Value> = k.read(|s| s.team.of(&p.id).into_iter().map(|w| json!({ "id": w.id, "name": w.name, "role": w.members.get(&p.id) })).collect());
    Ok(Json(v).into_response())
}

async fn create(State(k): K, h: HeaderMap, Json(b): Json<Named>) -> ApiResult {
    let p = who(&k, &h).await?;
    // Any regular user may start a shared project and collaborate as peers;
    // restricted guest accounts still cannot create new shared projects.
    if crate::people::rights(&p).held_to_guest_limits() {
        return refuse(StatusCode::FORBIDDEN, "guests cannot create a shared project");
    }
    if !short(&b.name) {
        return refuse(StatusCode::BAD_REQUEST, "a workspace needs a short name");
    }
    let id = new_id();
    send(&k, &p, Change::Created { workspace: id.clone(), name: b.name, owner: p.id.clone() }).await?;
    // Ready to collaborate at once — each collaborator's own personal Magnus
    // and personal computer do their work and share scrubbed outputs here.
    send(&k, &p, Change::Channel { workspace: id.clone(), channel: new_id(), name: FIRST_CHANNEL.into() }).await?;
    Ok(Json(json!({ "id": id })).into_response())
}

async fn one(State(k): K, Member { person: p, role, .. }: Member<at::Guest>, Path(ws): Path<String>) -> ApiResult {
    let seen = k.read(|s| {
        let v = MemberView::of(s, &ws, &p.id)?;
        let channels: Vec<Value> = v.channels().map(|(id, c)| json!({ "id": id, "name": c.name })).collect();
        Some((v.workspace().clone(), channels, v.files().cloned().collect::<Vec<File>>()))
    });
    let Some((w, channels, files)) = seen else { return refuse(StatusCode::NOT_FOUND, "no such workspace") };
    Ok(Json(json!({ "id": w.id, "name": w.name, "role": role, "members": w.members, "channels": channels, "connectors": w.connectors, "files": files, "knowledge": w.entries(None).iter().map(|e| (e.id.clone(), e.statement.clone())).collect::<BTreeMap<_, _>>(), "local_ceiling": w.local_ceiling, "routines": w.routines.iter().filter(|(_, r)| w.sees(&r.channel, &p.id)).collect::<BTreeMap<_, _>>(), "projects": w.projects.values().filter(|d| d.may_read(&p.id)).map(|d| json!({ "id": d.id, "name": d.name })).collect::<Vec<_>>(), "agent": { "id": super::conversation::AGENT_ID, "name": super::conversation::AGENT_NAME } })).into_response())
}

/// Delete the shared project: only its creator (or legacy workspace admin) may
/// delete it for everyone; any collaborator may leave on their own.
async fn remove(State(k): K, Member { person: p, .. }: Member<at::Admin>, Path(ws): Path<String>) -> ApiResult {
    if let Err(e) = k.delete_workspace(&format!("person:{}", p.id), &ws).await {
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"));
    }
    Ok(Json(json!({ "ok": true })).into_response())
}

fn default_member_role() -> Role {
    Role::Member
}

#[derive(Deserialize)]
struct SetMember {
    person: String,
    #[serde(default = "default_member_role")]
    role: Role,
}

async fn set_member(State(k): K, Member { person: p, role, .. }: Member<at::Member>, Path(ws): Path<String>, Json(b): Json<SetMember>) -> ApiResult {
    if k.person(&b.person).is_none() {
        return refuse(StatusCode::NOT_FOUND, "no such person");
    }
    membership(&k, &p, role, &ws, &b.person, Some(b.role)).await?;
    Ok(Json(json!({ "ok": true })).into_response())
}

/// An owner or admin removes a member; anyone leaves on their own.
async fn remove_member(State(k): K, Member { person: p, role, .. }: Member<at::Guest>, Path((ws, person)): Path<(String, String)>) -> ApiResult {
    allow(role, &p.id, Act::Remove { person: &person })?;
    if k.read(|s| s.team.role(&ws, &person)).is_none() {
        return refuse(StatusCode::NOT_FOUND, "they are not in this workspace");
    }
    membership(&k, &p, role, &ws, &person, None).await?;
    Ok(Json(json!({ "ok": true })).into_response())
}

/// A role given or a member taken out, once the rules of who may allow it.
async fn membership(k: &Kernel, p: &Person, mine: Role, ws: &str, person: &str, to: Option<Role>) -> Result<(), Box<Reply>> {
    let why = k.read(|s| s.team.workspace(ws).and_then(|w| super::membership::refuse(w, &p.id, mine, person, to)));
    if let Some(why) = why {
        return Err(Box::new(err(StatusCode::FORBIDDEN, why)));
    }
    k.set_workspace_role(&format!("person:{}", p.id), ws, person, to).await.map_err(|e| Box::new(err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))))
}

async fn add_channel(State(k): K, Member { person: p, .. }: Member<at::Member>, Path(ws): Path<String>, Json(b): Json<Named>) -> ApiResult {
    if !short(&b.name) {
        return refuse(StatusCode::BAD_REQUEST, "a channel needs a short name");
    }
    let id = new_id();
    send(&k, &p, Change::Channel { workspace: ws, channel: id.clone(), name: b.name }).await?;
    Ok(Json(json!({ "id": id })).into_response())
}

/// Whether the channel is there and this person sees it.
pub(super) fn has_channel(k: &Kernel, ws: &str, channel: &str, person: &str) -> bool {
    k.read(|s| MemberView::of(s, ws, person).is_some_and(|v| v.sees(channel)))
}

/// A channel's messages, with each finished request's answer in its place:
/// what was said, as the channel view shows it to this person.
async fn messages(State(k): K, Member { person: p, .. }: Member<at::Guest>, Path((ws, channel)): Path<(String, String)>) -> ApiResult {
    if !has_channel(&k, &ws, &channel, &p.id) {
        return refuse(StatusCode::NOT_FOUND, "no such channel");
    }
    let now = chrono::Utc::now();
    let said = k.read(|s| MemberView::of(s, &ws, &p.id).map(|v| super::items::said(&v, &channel, now)).unwrap_or_default());
    Ok(Json(said).into_response())
}

/// A message in a shared conversation: what is said, in the shape every
/// place says it (`say::Say`: text, the person's own files, the model as
/// picked with its reasoning), and where and to whom.
#[derive(Deserialize)]
struct NewMessage {
    #[serde(flatten)]
    say: crate::say::Say,
    #[serde(default)]
    thread: Option<String>,
    /// Asks the sender's personal Magnus to help and share its output.
    #[serde(default)]
    agent: bool,
    /// For the sender's own agent only, and it does not reach the channel.
    /// It alone carries the person's own files: a message to the channel
    /// names files already in the shared Files.
    #[serde(default)]
    whisper: bool,
}

async fn post_message(State(k): K, Member { person: p, role, .. }: Member<at::Guest>, Path((ws, channel)): Path<(String, String)>, Json(b): Json<NewMessage>) -> ApiResult {
    if !has_channel(&k, &ws, &channel, &p.id) {
        return refuse(StatusCode::NOT_FOUND, "no such channel");
    }
    if b.say.text.trim().is_empty() || b.say.text.chars().count() > MAX_MESSAGE_CHARS {
        return refuse(StatusCode::BAD_REQUEST, "a message needs text, and not too much of it");
    }
    allow(role, &p.id, Act::Post { with_agent: b.agent || b.whisper })?;
    if !b.whisper && !b.say.files.is_empty() {
        return refuse(StatusCode::BAD_REQUEST, "a message here names files in the shared Files; only a whisper takes files of your own");
    }
    let said = b.say.check(&k, &p).await?;
    if b.whisper {
        return Ok(match k.whisper(&ws, &channel, b.thread, &p.id, said).await {
            Ok((task, run)) => Json(json!({ "task_id": task, "run_id": run })).into_response(),
            Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
        });
    }
    Ok(post_and_ask(&k, &p, role, &ws, &channel, b.thread, said.text, b.agent, said.pick).await)
}

/// Post a message as `p`, and when asked or `@Magnus` is mentioned, dispatch
/// `p`'s own personal Magnus via the Mission + Release Airlock engine so each
/// collaborator's agent does their work in isolation and shares scrubbed output.
#[allow(clippy::too_many_arguments)]
pub(super) async fn post_and_ask(
    k: &Arc<Kernel>,
    p: &Person,
    role: Role,
    ws: &str,
    channel: &str,
    thread: Option<String>,
    text: String,
    asked: bool,
    pick: crate::say::Pick,
) -> Reply {
    super::presence::sent(ws, channel, &p.id);
    let message = Message { id: new_id(), sender: p.id.clone(), text: text.clone(), thread: thread.clone(), at: chrono::Utc::now() };
    let id = message.id.clone();
    ok_or_return!(send(k, p, Change::Message { workspace: ws.to_string(), channel: channel.to_string(), message }).await);
    let why = if asked { Some(Why::Asked) } else { k.read(|s| s.team.workspace(ws).and_then(|w| w.agent_answers(channel, role, &text))) };
    let Some(why) = why else { return Json(json!({ "id": id })).into_response() };
    let objective: String = text.trim().chars().take(super::orchestrator::MAX_OBJECTIVE_CHARS).collect();
    if let Some(reason) = crate::compartment::query_gate::scrub_check(&objective) {
        return Json(json!({ "id": id, "limited": reason })).into_response();
    }
    // Called on (asked, @mentioned, a direct chat) the answer goes to the
    // room; only named or cued, it waits in the sender's Airlock.
    let hold = matches!(why, Why::Named | Why::Helpful);
    match k.start_personal_room_mission(ws, channel, &p.id, &objective, thread, pick, hold).await {
        Ok((mission, task, run)) => {
            ok_or_return!(
                send(k, p, Change::Chat { workspace: ws.to_string(), chat: Chat::Asked { channel: channel.to_string(), message: id.clone(), task: task.clone() } }).await
            );
            Json(json!({ "id": id, "mission_id": mission, "task_id": task, "run_id": run, "agent": why })).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}

#[derive(Deserialize)]
struct GrantConnector {
    app: String,
    roles: Vec<Role>,
}

async fn grant_connector(State(k): K, Member { person: p, .. }: Member<at::Admin>, Path(ws): Path<String>, Json(b): Json<GrantConnector>) -> ApiResult {
    if !k.connectors().iter().any(|c| c.name == b.app) {
        return refuse(StatusCode::NOT_FOUND, "no such app");
    }
    send(&k, &p, Change::Connector { workspace: ws, app: b.app, roles: b.roles }).await?;
    Ok(Json(json!({ "ok": true })).into_response())
}

async fn read_file(State(k): K, Member { person: p, .. }: Member<at::Guest>, Path((ws, name)): Path<(String, String)>) -> ApiResult {
    let known = k.read(|s| s.team.workspace(&ws).is_some_and(|w| w.sees_file(&name, &p.id)));
    if !known || !fit_name(&name) {
        return refuse(StatusCode::NOT_FOUND, "no such file");
    }
    Ok(match tokio::fs::read(volume_dir(&k, &ws).join(&name)).await {
        Ok(bytes) => bytes.into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")),
    })
}

/// An explicit share: the only way a member's file enters the volume.
async fn write_file(
    State(k): K,
    Member { person: p, .. }: Member<at::Member>,
    Path((ws, name)): Path<(String, String)>,
    q: Query<HashMap<String, String>>,
    body: Bytes,
) -> ApiResult {
    if !fit_name(&name) {
        return refuse(StatusCode::BAD_REQUEST, "that is not a file name");
    }
    if k.read(|s| s.team.workspace(&ws).is_some_and(|w| super::keep::someone_elses(w, &name, &p.id))) {
        return refuse(StatusCode::CONFLICT, super::keep::SOMEONE_ELSES);
    }
    let dir = volume_dir(&k, &ws);
    if let Err(e) = async {
        tokio::fs::create_dir_all(&dir).await?;
        tokio::fs::write(dir.join(&name), &body).await
    }
    .await
    {
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}"));
    }
    let project = q.get("project").filter(|id| k.read(|s| s.team.workspace(&ws).is_some_and(|w| w.projects.get(*id).is_some_and(|d| d.may_read(&p.id))))).cloned();
    let file = File { name: name.clone(), size: body.len() as u64, by: p.id.clone() };
    send(&k, &p, Change::File { workspace: ws.clone(), file }).await?;
    if project.is_some() {
        send(&k, &p, Change::Chat { workspace: ws, chat: Chat::FileIn { name, project } }).await?;
    }
    Ok(Json(json!({ "ok": true })).into_response())
}

/// Whoever put it there, or an owner or admin, may delete a file.
async fn delete_file(State(k): K, Member { person: p, role, .. }: Member<at::Member>, Path((ws, name)): Path<(String, String)>) -> ApiResult {
    let by = k.read(|s| s.team.workspace(&ws).filter(|w| w.sees_file(&name, &p.id)).and_then(|w| w.files.get(&name).map(|f| f.by.clone())));
    let Some(by) = by else { return refuse(StatusCode::NOT_FOUND, "no such file") };
    allow(role, &p.id, Act::DeleteFile { by: &by })?;
    if fit_name(&name) {
        if let Err(e) = tokio::fs::remove_file(volume_dir(&k, &ws).join(&name)).await {
            tracing::warn!("{name}: {e}");
        }
    }
    send(&k, &p, Change::FileRemoved { workspace: ws, name }).await?;
    Ok(Json(json!({ "ok": true })).into_response())
}

/// The requester's own choice to put a held file on the team volume. The
/// team gets a copy; theirs goes to their own library, so an admin deleting
/// the team's copy, or the workspace, leaves it with them.
async fn share_held(State(k): K, Member { person: p, .. }: Member<at::Member>, Path((ws, name)): Path<(String, String)>) -> ApiResult {
    let held = k.read(|s| s.team.workspace(&ws).and_then(|w| w.held.get(&p.id).and_then(|f| f.get(&name).cloned())));
    let Some(file) = held else { return refuse(StatusCode::NOT_FOUND, "you hold no such file") };
    if k.read(|s| s.team.workspace(&ws).is_some_and(|w| super::keep::someone_elses(w, &name, &p.id))) {
        return refuse(StatusCode::CONFLICT, super::keep::SOMEONE_ELSES);
    }
    let from = held_dir(&k, &ws, &p.id).join(&name);
    let bytes = match tokio::fs::read(&from).await {
        Ok(b) => b,
        Err(e) => return refuse(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")),
    };
    if let Err(e) = k.keep_in_library(&format!("person:{}", p.id), &p.id, &name, &bytes).await {
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"));
    }
    let target = volume_dir(&k, &ws);
    if let Err(e) = async {
        tokio::fs::create_dir_all(&target).await?;
        tokio::fs::rename(&from, target.join(&name)).await
    }
    .await
    {
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}"));
    }
    send(&k, &p, Change::File { workspace: ws.clone(), file }).await?;
    send(&k, &p, Change::Released { workspace: ws, person: p.id.clone(), name }).await?;
    Ok(Json(json!({ "ok": true })).into_response())
}
