//! The workspace as a team chat over HTTP: the left rail, a channel as it
//! reads now, editing, reactions, pins, per-channel settings, direct
//! conversations, read markers, typing and the activity view.
use super::caller::{at, Member};
use super::chat::{AgentMode, Chat, Direct, Notify};
use super::conversation::{AGENT_ID, AGENT_NAME};
use super::items;
use super::may::{allow, Act};
use super::member_view::MemberView;
use super::routes::{has_channel, new_id, send, Reply, K};
use super::{presence, Change, Workspace};
use crate::api_error::{refuse, ApiResult};
use crate::config::Person;
use crate::conversation::{channel as conv, Conversation, NO_SUCH_MESSAGE};
use crate::kernel::Kernel;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

const MAX_MESSAGE_CHARS: usize = 20_000;
const MAX_EMOJI_CHARS: usize = 16;
const MAX_ACTIVITY_ITEMS: usize = 100;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/workspaces/{ws}/home", get(home))
        .route("/v1/workspaces/{ws}/activity", get(activity))
        .route("/v1/workspaces/{ws}/direct", post(direct))
        .route("/v1/workspaces/{ws}/channels/{channel}/view", get(view))
        .route("/v1/workspaces/{ws}/channels/{channel}/messages/{id}", put(edit).delete(super::own_view::unsend))
        .route("/v1/workspaces/{ws}/channels/{channel}/messages/{id}/react", post(react))
        .route("/v1/workspaces/{ws}/channels/{channel}/messages/{id}/pin", post(pin))
        .route("/v1/workspaces/{ws}/channels/{channel}/messages/{id}/export", get(export))
        .route("/v1/workspaces/{ws}/channels/{channel}/agent", post(set_mode))
        .route("/v1/workspaces/{ws}/channels/{channel}/notify", post(set_notify))
        .route("/v1/workspaces/{ws}/channels/{channel}/read", post(read))
        .route("/v1/workspaces/{ws}/channels/{channel}/typing", post(typing))
}

pub(super) fn name_of(k: &Kernel, id: &str) -> String {
    if id == AGENT_ID {
        return AGENT_NAME.to_string();
    }
    k.people().name(id)
}

fn workspace(k: &Kernel, ws: &str) -> Option<Workspace> {
    k.read(|s| s.team.workspace(ws).cloned())
}

/// Everything the left rail needs, for this person.
async fn home(State(k): K, Member { person: p, role, .. }: Member<at::Guest>, Path(ws): Path<String>) -> ApiResult {
    presence::seen(&ws, &p.id);
    let Some(w) = workspace(&k, &ws) else { return refuse(StatusCode::NOT_FOUND, "no such workspace") };
    let online = presence::online(&ws);
    let me = name_of(&k, &p.id);
    let members: Vec<Value> = w.members.iter().map(|(id, r)| json!({ "id": id, "name": name_of(&k, id), "role": r, "online": online.contains(id) })).collect();
    let channels: Vec<Value> = w
        .channels
        .iter()
        .filter(|(id, _)| w.sees(id, &p.id))
        .map(|(id, c)| {
            let (unread, mentions) = w.unread(id, &p.id, &me);
            let direct = w.chat.direct.get(id).map(|d| json!({ "people": d.people, "agent": d.agent }));
            let (archived, deleted) = (w.archived(id, &p.id), w.deleted_for(id, &p.id));
            json!({ "id": id, "name": c.name, "project": c.project, "mode": w.agent_mode(id), "notify": w.notify_level(id, &p.id), "unread": unread, "mentions": mentions, "direct": direct, "archived": archived, "deleted": deleted })
        })
        .collect();
    let projects: Vec<Value> = w
        .projects
        .values()
        .filter(|d| d.may_read(&p.id))
        .map(|d| {
            let (done, total) = w.progress(&d.id);
            let channels: Vec<&String> = w.channels.iter().filter(|(_, c)| c.project.as_deref() == Some(&d.id)).map(|(id, _)| id).collect();
            json!({ "id": d.id, "name": d.name, "owner": d.owner, "members": d.members, "progress": { "done": done, "total": total }, "channels": channels })
        })
        .collect();
    let spent = k.read(|s| super::spend::spent_this_month(s, &ws, chrono::Utc::now()));
    Ok(Json(json!({
        "id": w.id, "name": w.name, "role": role, "me": p.id, "me_name": me,
        "agent": { "id": AGENT_ID, "name": AGENT_NAME },
        "members": members, "channels": channels, "projects": projects,
        "computer": k.team_computer(&ws), "spend": super::spend::summary(&w, &spent),
        "apps": k.workspace_app_list(&w),
    }))
    .into_response())
}

/// An item's time, for ordering: its text form does not sort.
pub(super) fn when(item: &Value) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    item["at"].as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
}

/// One message as a Markdown file, as this person sees it: the team chat
/// has the personal chat's message controls, Export among them (owner,
/// 2026-11-13).
async fn export(State(k): K, Member { person: p, .. }: Member<at::Guest>, Path((ws, channel, id)): Path<(String, String, String)>) -> ApiResult {
    let names = conv::names(&k, &ws);
    conv::read(&k, &ws, &p.id, &channel, &names, |c| {
        c.entry(&id).and_then(|e| if c.text(&e).is_empty() { refuse(StatusCode::NOT_FOUND, NO_SUCH_MESSAGE) } else { c.export(&e.id) })
    })
}

/// A channel as it reads now: messages with their reactions, edits, pins
/// and reply counts; the agent's work and answers; who is typing.
async fn view(State(k): K, Member { person: p, role, .. }: Member<at::Guest>, Path((ws, channel)): Path<(String, String)>) -> ApiResult {
    if !has_channel(&k, &ws, &channel, &p.id) {
        return refuse(StatusCode::NOT_FOUND, "no such channel");
    }
    presence::seen(&ws, &p.id);
    let now = chrono::Utc::now();
    let none = conv::Names::new();
    let items = k.read(|s| MemberView::of(s, &ws, &p.id).map(|view| conv::Channel { view, channel: &channel, thread: None, now, names: &none }.entries()).unwrap_or_default());
    let working = k.read(|s| s.team.runs_in(&ws, &channel).iter().any(|r| s.tasks.get(&r.task).and_then(|t| s.runs.get(&t.run_id)).is_some_and(|run| !run.status.terminal())));
    let mut typing: Vec<String> = presence::typing_in(&ws, &channel).into_iter().filter(|t| t != &p.id).map(|t| name_of(&k, &t)).collect();
    if working {
        typing.push(AGENT_NAME.to_string());
    }
    let names: BTreeMap<String, String> = items.iter().flat_map(|i| i.sender.iter().chain(i.requester.as_ref())).map(|id| (id.clone(), name_of(&k, id))).collect();
    let caps = conv::Channel::<crate::state::State>::caps_for(role);
    Ok(Json(json!({ "messages": items::for_the_app(items, &names), "typing": typing, "names": names, "caps": caps })).into_response())
}

#[derive(Deserialize)]
struct Edit {
    text: String,
}

/// Only who wrote a message edits it.
async fn edit(State(k): K, Member { person: p, .. }: Member<at::Guest>, Path((ws, channel, id)): Path<(String, String, String)>, Json(b): Json<Edit>) -> ApiResult {
    // One deleted for everyone stays deleted: an edit would put words back.
    super::may::check(&k, &ws, &channel, &id, &p.id, |m| m.edit, "only who wrote a message edits it")?;
    if b.text.trim().is_empty() || b.text.chars().count() > MAX_MESSAGE_CHARS {
        return refuse(StatusCode::BAD_REQUEST, "a message needs text, and not too much of it");
    }
    send(&k, &p, Change::Chat { workspace: ws, chat: Chat::Edited { channel, id, text: b.text, at: chrono::Utc::now() } }).await?;
    Ok(Json(json!({ "ok": true })).into_response())
}

#[derive(Deserialize)]
struct React {
    emoji: String,
    on: bool,
}

async fn react(State(k): K, Member { person: p, .. }: Member<at::Guest>, Path((ws, channel, id)): Path<(String, String, String)>, Json(b): Json<React>) -> ApiResult {
    if !has_channel(&k, &ws, &channel, &p.id) {
        return refuse(StatusCode::NOT_FOUND, "no such channel");
    }
    if b.emoji.trim().is_empty() || b.emoji.chars().count() > MAX_EMOJI_CHARS {
        return refuse(StatusCode::BAD_REQUEST, "a reaction is one emoji");
    }
    send(&k, &p, Change::Chat { workspace: ws, chat: Chat::Reacted { channel, id, emoji: b.emoji, person: p.id.clone(), on: b.on } }).await?;
    Ok(Json(json!({ "ok": true })).into_response())
}

#[derive(Deserialize)]
struct Pin {
    pinned: bool,
}

async fn pin(State(k): K, Member { person: p, .. }: Member<at::Member>, Path((ws, channel, id)): Path<(String, String, String)>, Json(b): Json<Pin>) -> ApiResult {
    if !has_channel(&k, &ws, &channel, &p.id) {
        return refuse(StatusCode::NOT_FOUND, "no such channel");
    }
    super::may::check(&k, &ws, &channel, &id, &p.id, |m| m.pin, "a guest pins nothing")?;
    send(&k, &p, Change::Chat { workspace: ws, chat: Chat::Pinned { channel, id, pinned: b.pinned } }).await?;
    Ok(Json(json!({ "ok": true })).into_response())
}

#[derive(Deserialize)]
struct Mode {
    mode: AgentMode,
}

/// Any member sets how the team agent takes part in a channel.
async fn set_mode(State(k): K, Member { person: p, .. }: Member<at::Member>, Path((ws, channel)): Path<(String, String)>, Json(b): Json<Mode>) -> ApiResult {
    if !has_channel(&k, &ws, &channel, &p.id) {
        return refuse(StatusCode::NOT_FOUND, "no such channel");
    }
    send(&k, &p, Change::Chat { workspace: ws, chat: Chat::AgentMode { channel, mode: b.mode } }).await?;
    Ok(Json(json!({ "ok": true })).into_response())
}

#[derive(Deserialize)]
struct Level {
    level: Notify,
}

async fn set_notify(State(k): K, Member { person: p, .. }: Member<at::Guest>, Path((ws, channel)): Path<(String, String)>, Json(b): Json<Level>) -> ApiResult {
    if !has_channel(&k, &ws, &channel, &p.id) {
        return refuse(StatusCode::NOT_FOUND, "no such channel");
    }
    send(&k, &p, Change::Chat { workspace: ws, chat: Chat::Notify { channel, person: p.id.clone(), level: b.level } }).await?;
    Ok(Json(json!({ "ok": true })).into_response())
}

/// Marks the channel read up to its newest message, once per change.
async fn read(State(k): K, Member { person: p, .. }: Member<at::Guest>, Path((ws, channel)): Path<(String, String)>) -> ApiResult {
    if !has_channel(&k, &ws, &channel, &p.id) {
        return refuse(StatusCode::NOT_FOUND, "no such channel");
    }
    let (newest, mark) = k.read(|s| {
        let w = s.team.workspace(&ws);
        let newest = w.and_then(|w| w.channels.get(&channel)?.messages.iter().map(|m| m.at).max());
        (newest, w.and_then(|w| w.chat.read.get(&p.id)?.get(&channel).copied()))
    });
    if let Some(at) = newest.filter(|n| mark.is_none_or(|m| *n > m)) {
        send(&k, &p, Change::Chat { workspace: ws, chat: Chat::Read { channel, person: p.id.clone(), at } }).await?;
    }
    Ok(Json(json!({ "ok": true })).into_response())
}

async fn typing(State(k): K, Member { person: p, .. }: Member<at::Guest>, Path((ws, channel)): Path<(String, String)>) -> ApiResult {
    if has_channel(&k, &ws, &channel, &p.id) {
        presence::typing(&ws, &channel, &p.id);
    }
    Ok(Json(json!({ "ok": true })).into_response())
}

#[derive(Deserialize)]
struct NewDirect {
    #[serde(default)]
    people: Vec<String>,
    /// A conversation with the team agent.
    #[serde(default)]
    agent: bool,
}

/// The direct conversation between these people, made the first time.
async fn direct(State(k): K, Member { person: p, role, .. }: Member<at::Guest>, Path(ws): Path<String>, Json(b): Json<NewDirect>) -> ApiResult {
    let Some(w) = workspace(&k, &ws) else { return refuse(StatusCode::NOT_FOUND, "no such workspace") };
    let mut people: Vec<String> = b.people.into_iter().filter(|x| x != &p.id).collect();
    people.push(p.id.clone());
    people.sort();
    people.dedup();
    if !people.iter().all(|x| w.members.contains_key(x)) {
        return refuse(StatusCode::BAD_REQUEST, "a direct message is between members");
    }
    allow(role, &p.id, Act::Post { with_agent: b.agent })?;
    let id = direct_between(&k, &p, &w, people, b.agent).await?;
    Ok(Json(json!({ "id": id })).into_response())
}

/// The direct conversation between these people (the caller among them),
/// made the first time.
pub(super) async fn direct_between(k: &Kernel, p: &Person, w: &Workspace, people: Vec<String>, agent: bool) -> Result<String, Box<Reply>> {
    let found = Direct { people: people.clone(), agent };
    if let Some((id, _)) = w.chat.direct.iter().find(|(_, d)| **d == found) {
        return Ok(id.clone());
    }
    let id = new_id();
    send(k, p, Change::Channel { workspace: w.id.clone(), channel: id.clone(), name: String::new() }).await?;
    send(k, p, Change::Chat { workspace: w.id.clone(), chat: Chat::Direct { channel: id.clone(), people, agent } }).await?;
    Ok(id)
}

/// Mentions of this person, replies in threads they took part in, and their
/// requests waiting on an answer from them.
async fn activity(State(k): K, Member { person: p, .. }: Member<at::Guest>, Path(ws): Path<String>) -> ApiResult {
    let me = name_of(&k, &p.id);
    let out = k.read(|s| {
        let Some(v) = MemberView::of(s, &ws, &p.id) else { return json!({}) };
        let (mut mentions, mut replies) = (vec![], vec![]);
        for (cid, ch) in v.channels() {
            let mine: Vec<&str> = ch.messages.iter().filter(|m| m.sender == p.id).map(|m| m.thread.as_deref().unwrap_or(&m.id)).collect();
            for m in v.messages(cid).into_iter().filter(|m| m.sender != p.id) {
                let item = json!({ "channel": cid, "channel_name": ch.name, "id": m.id, "sender": m.sender, "text": m.text, "thread": m.thread, "at": m.at });
                if super::chat::mentions(&m.text, &p.id, &me) {
                    mentions.push(item);
                } else if m.thread.as_deref().is_some_and(|t| mine.contains(&t)) {
                    replies.push(item);
                }
            }
        }
        let asks: Vec<Value> = s
            .team
            .runs_by(&ws, &p.id)
            .into_iter()
            .filter_map(|r| s.tasks.get(&r.task).and_then(|t| s.runs.get(&t.run_id)).filter(|run| run.status == crate::state::Status::Suspended).map(|run| (r, run)))
            .map(|(r, run)| json!({ "task": r.task, "channel": r.channel, "reason": run.reason }))
            .collect();
        for list in [&mut mentions, &mut replies] {
            list.sort_by_key(|i| std::cmp::Reverse(when(i)));
            list.truncate(MAX_ACTIVITY_ITEMS);
        }
        json!({ "mentions": mentions, "replies": replies, "asks": asks })
    });
    Ok(Json(out).into_response())
}
