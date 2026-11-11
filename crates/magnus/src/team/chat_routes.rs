//! The workspace as a team chat over HTTP: the left rail, a channel as it
//! reads now, editing, reactions, pins, per-channel settings, direct
//! conversations, read markers, typing and the activity view.
use super::chat::{AgentMode, Chat, Direct, Notify};
use super::conversation::{is_long, placement, AGENT_ID, AGENT_NAME};
use super::routes::{has_channel, member, new_id, send, Reply, K};
use super::{presence, Change, Message, Role, Workspace};
use crate::api::err;
use crate::kernel::Kernel;
use crate::state::State as Shared;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
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
const MAX_TITLE_CHARS: usize = 80;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/workspaces/{ws}/home", get(home))
        .route("/v1/workspaces/{ws}/activity", get(activity))
        .route("/v1/workspaces/{ws}/direct", post(direct))
        .route("/v1/workspaces/{ws}/channels/{channel}/view", get(view))
        .route("/v1/workspaces/{ws}/channels/{channel}/messages/{id}", put(edit).delete(super::own_view::unsend))
        .route("/v1/workspaces/{ws}/channels/{channel}/messages/{id}/react", post(react))
        .route("/v1/workspaces/{ws}/channels/{channel}/messages/{id}/pin", post(pin))
        .route("/v1/workspaces/{ws}/channels/{channel}/agent", post(set_mode))
        .route("/v1/workspaces/{ws}/channels/{channel}/notify", post(set_notify))
        .route("/v1/workspaces/{ws}/channels/{channel}/read", post(read))
        .route("/v1/workspaces/{ws}/channels/{channel}/typing", post(typing))
}

pub(super) fn name_of(k: &Kernel, id: &str) -> String {
    if id == AGENT_ID {
        return AGENT_NAME.to_string();
    }
    k.person(id).map(|p| p.name).filter(|n| !n.is_empty()).unwrap_or_else(|| id.to_string())
}

fn workspace(k: &Kernel, ws: &str) -> Option<Workspace> {
    k.read(|s| s.team.workspace(ws).cloned())
}

/// Everything the left rail needs, for this person.
async fn home(State(k): K, h: HeaderMap, Path(ws): Path<String>) -> Reply {
    let (p, role) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    presence::seen(&ws, &p.id);
    let Some(w) = workspace(&k, &ws) else { return err(StatusCode::NOT_FOUND, "no such workspace") };
    let online = presence::online(&ws);
    let me = name_of(&k, &p.id);
    let members: Vec<Value> = w.members.iter().map(|(id, r)| json!({ "id": id, "name": name_of(&k, id), "role": r, "online": online.contains(id) })).collect();
    let channels: Vec<Value> = w
        .channels
        .iter()
        .filter(|(id, c)| w.sees(id, &p.id) && w.in_project(c.project.as_deref(), &p.id))
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
    Json(json!({
        "id": w.id, "name": w.name, "role": role, "me": p.id, "me_name": me,
        "agent": { "id": AGENT_ID, "name": AGENT_NAME },
        "members": members, "channels": channels, "projects": projects,
        "computer": k.team_computer(&ws), "spend": super::spend::summary(&w, &spent),
    }))
    .into_response()
}

fn status_word(s: crate::state::Status) -> &'static str {
    use crate::state::Status::*;
    match s {
        Running => "working",
        Suspended => "waiting",
        Completed => "done",
        Failed => "failed",
        Cancelled => "stopped",
    }
}

/// The last tool the run called: what it is doing now.
fn doing(run: &crate::state::Run) -> Option<String> {
    run.replies.iter().rev().flat_map(|r| r.blocks.iter().rev()).find_map(|b| match b {
        magnus_adapter_proto::Block::ToolCall { name, .. } => Some(name.clone()),
        _ => None,
    })
}

/// Each request's live card and finished answer, in the channel or the
/// thread they belong in.
fn agent_items(s: &Shared, w: &Workspace, channel: &str, viewer: &str, now: chrono::DateTime<chrono::Utc>) -> (Vec<Value>, BTreeMap<String, Value>) {
    let (mut items, mut cards) = (vec![], BTreeMap::new());
    let texts: BTreeMap<&str, &str> = w.channels.get(channel).map(|c| c.messages.iter().map(|m| (m.id.as_str(), m.text.as_str())).collect()).unwrap_or_default();
    for r in s.team.runs_in(&w.id, channel) {
        let Some(task) = s.tasks.get(&r.task) else { continue };
        let Some(run) = s.runs.get(&task.run_id) else { continue };
        let steps = run.replies.len();
        let long = is_long(task.created_at, run.ended_at.unwrap_or(now), steps);
        let asked = w.chat.asked.get(&r.task).map(String::as_str);
        let thread = placement(r.thread.as_deref(), asked, long);
        let title: String = asked.and_then(|m| texts.get(m)).map_or("Following up", |t| t).chars().take(MAX_TITLE_CHARS).collect();
        let card = json!({
            "task": r.task, "requester": r.requester, "status": status_word(run.status), "steps": steps,
            "doing": doing(run), "title": title, "long": long, "thread": thread, "started": task.created_at, "reason": run.reason,
        });
        match asked {
            Some(m) => {
                cards.insert(m.to_string(), card);
            }
            None => items.push(
                json!({ "id": format!("work:{}", r.task), "sender": AGENT_ID, "requester": r.requester, "text": "", "thread": r.thread, "at": task.created_at, "work": card }),
            ),
        }
        let answer = match run.status {
            crate::state::Status::Completed => run.summary.clone(),
            crate::state::Status::Failed => Some(format!("I could not finish this: {}", run.reason.as_deref().unwrap_or("it failed"))),
            _ => None,
        };
        if let Some(text) = answer {
            let (text, private) = super::answer_for(run, text, &r.requester, viewer);
            let at = run.ended_at.unwrap_or(now);
            items.push(json!({ "id": format!("agent:{}", r.task), "sender": AGENT_ID, "requester": r.requester, "task": r.task, "text": text, "thread": thread, "at": at, "private": private }));
        }
    }
    (items, cards)
}

/// An item's time, for ordering: its text form does not sort.
pub(super) fn when(item: &Value) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    item["at"].as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
}

fn message_item(w: &Workspace, m: &Message) -> Value {
    let reactions = w.chat.reactions.get(&m.id).cloned().unwrap_or_default();
    let (sender, requester) = match m.sender.strip_prefix("agent:") {
        Some(r) => (AGENT_ID.to_string(), Some(r.to_string())),
        None => (m.sender.clone(), None),
    };
    let deleted = w.chat.unsent.contains(&m.id);
    json!({ "id": m.id, "sender": sender, "requester": requester, "text": m.text, "thread": m.thread, "at": m.at, "edited": w.chat.edited.contains(&m.id), "pinned": w.chat.pins.contains(&m.id), "reactions": reactions, "deleted": deleted })
}

/// A channel as it reads now: messages with their reactions, edits, pins
/// and reply counts; the agent's work and answers; who is typing.
async fn view(State(k): K, h: HeaderMap, Path((ws, channel)): Path<(String, String)>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    if !has_channel(&k, &ws, &channel, &p.id) {
        return err(StatusCode::NOT_FOUND, "no such channel");
    }
    presence::seen(&ws, &p.id);
    let now = chrono::Utc::now();
    let (mut items, working) = k.read(|s| {
        let Some(w) = s.team.workspace(&ws) else { return (vec![], false) };
        let (agent, cards) = agent_items(s, w, &channel, &p.id, now);
        let mut items: Vec<Value> = w.channels.get(&channel).map(|c| c.messages.iter().map(|m| message_item(w, m)).collect()).unwrap_or_default();
        for item in &mut items {
            if let Some(card) = item["id"].as_str().and_then(|id| cards.get(id)) {
                item["work"] = card.clone();
            }
        }
        items.extend(agent);
        let working = s.team.runs_in(&ws, &channel).iter().any(|r| s.tasks.get(&r.task).and_then(|t| s.runs.get(&t.run_id)).is_some_and(|run| !run.status.terminal()));
        (items, working)
    });
    let cleared = k.read(|s| s.team.workspace(&ws).and_then(|w| w.cleared(&channel, &p.id)));
    items.retain(|i| cleared.is_none_or(|c| when(i).is_some_and(|at| at > c)));
    items.sort_by_key(when);
    let mut replies: BTreeMap<String, (usize, Value)> = BTreeMap::new();
    for i in items.iter().filter(|i| i["thread"].is_string()) {
        let e = replies.entry(i["thread"].as_str().unwrap_or_default().to_string()).or_insert((0, Value::Null));
        e.0 += 1;
        e.1 = i["at"].clone();
    }
    for i in &mut items {
        if let Some((n, last)) = i["id"].as_str().and_then(|id| replies.get(id)) {
            i["replies"] = json!(n);
            i["last_reply"] = last.clone();
        }
    }
    // A deleted message with no replies leaves nothing behind.
    items.retain(|i| i["deleted"] != json!(true) || i["replies"].is_number());
    let mut typing: Vec<String> = presence::typing_in(&ws, &channel).into_iter().filter(|t| t != &p.id).map(|t| name_of(&k, &t)).collect();
    if working {
        typing.push(AGENT_NAME.to_string());
    }
    let names: BTreeMap<String, String> =
        items.iter().filter_map(|i| i["sender"].as_str()).chain(items.iter().filter_map(|i| i["requester"].as_str())).map(|id| (id.to_string(), name_of(&k, id))).collect();
    Json(json!({ "messages": items, "typing": typing, "names": names })).into_response()
}

#[derive(Deserialize)]
struct Edit {
    text: String,
}

/// Only who wrote a message edits it.
async fn edit(State(k): K, h: HeaderMap, Path((ws, channel, id)): Path<(String, String, String)>, Json(b): Json<Edit>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    let by = k
        .read(|s| s.team.workspace(&ws).filter(|w| w.sees(&channel, &p.id)).and_then(|w| w.channels.get(&channel)?.messages.iter().find(|m| m.id == id).map(|m| m.sender.clone())));
    let Some(by) = by else { return err(StatusCode::NOT_FOUND, "no such message") };
    if by != p.id {
        return err(StatusCode::FORBIDDEN, "only who wrote a message edits it");
    }
    if b.text.trim().is_empty() || b.text.chars().count() > MAX_MESSAGE_CHARS {
        return err(StatusCode::BAD_REQUEST, "a message needs text, and not too much of it");
    }
    ok_or_return!(send(&k, &p, Change::Chat { workspace: ws, chat: Chat::Edited { channel, id, text: b.text, at: chrono::Utc::now() } }).await);
    Json(json!({ "ok": true })).into_response()
}

#[derive(Deserialize)]
struct React {
    emoji: String,
    on: bool,
}

async fn react(State(k): K, h: HeaderMap, Path((ws, channel, id)): Path<(String, String, String)>, Json(b): Json<React>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    if !has_channel(&k, &ws, &channel, &p.id) {
        return err(StatusCode::NOT_FOUND, "no such channel");
    }
    if b.emoji.trim().is_empty() || b.emoji.chars().count() > MAX_EMOJI_CHARS {
        return err(StatusCode::BAD_REQUEST, "a reaction is one emoji");
    }
    ok_or_return!(send(&k, &p, Change::Chat { workspace: ws, chat: Chat::Reacted { channel, id, emoji: b.emoji, person: p.id.clone(), on: b.on } }).await);
    Json(json!({ "ok": true })).into_response()
}

#[derive(Deserialize)]
struct Pin {
    pinned: bool,
}

async fn pin(State(k): K, h: HeaderMap, Path((ws, channel, id)): Path<(String, String, String)>, Json(b): Json<Pin>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    if !has_channel(&k, &ws, &channel, &p.id) {
        return err(StatusCode::NOT_FOUND, "no such channel");
    }
    ok_or_return!(send(&k, &p, Change::Chat { workspace: ws, chat: Chat::Pinned { channel, id, pinned: b.pinned } }).await);
    Json(json!({ "ok": true })).into_response()
}

#[derive(Deserialize)]
struct Mode {
    mode: AgentMode,
}

/// Any member sets how the team agent takes part in a channel.
async fn set_mode(State(k): K, h: HeaderMap, Path((ws, channel)): Path<(String, String)>, Json(b): Json<Mode>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    if !has_channel(&k, &ws, &channel, &p.id) {
        return err(StatusCode::NOT_FOUND, "no such channel");
    }
    ok_or_return!(send(&k, &p, Change::Chat { workspace: ws, chat: Chat::AgentMode { channel, mode: b.mode } }).await);
    Json(json!({ "ok": true })).into_response()
}

#[derive(Deserialize)]
struct Level {
    level: Notify,
}

async fn set_notify(State(k): K, h: HeaderMap, Path((ws, channel)): Path<(String, String)>, Json(b): Json<Level>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    if !has_channel(&k, &ws, &channel, &p.id) {
        return err(StatusCode::NOT_FOUND, "no such channel");
    }
    ok_or_return!(send(&k, &p, Change::Chat { workspace: ws, chat: Chat::Notify { channel, person: p.id.clone(), level: b.level } }).await);
    Json(json!({ "ok": true })).into_response()
}

/// Marks the channel read up to its newest message, once per change.
async fn read(State(k): K, h: HeaderMap, Path((ws, channel)): Path<(String, String)>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    if !has_channel(&k, &ws, &channel, &p.id) {
        return err(StatusCode::NOT_FOUND, "no such channel");
    }
    let (newest, mark) = k.read(|s| {
        let w = s.team.workspace(&ws);
        let newest = w.and_then(|w| w.channels.get(&channel)?.messages.iter().map(|m| m.at).max());
        (newest, w.and_then(|w| w.chat.read.get(&p.id)?.get(&channel).copied()))
    });
    if let Some(at) = newest.filter(|n| mark.is_none_or(|m| *n > m)) {
        ok_or_return!(send(&k, &p, Change::Chat { workspace: ws, chat: Chat::Read { channel, person: p.id.clone(), at } }).await);
    }
    Json(json!({ "ok": true })).into_response()
}

async fn typing(State(k): K, h: HeaderMap, Path((ws, channel)): Path<(String, String)>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    if has_channel(&k, &ws, &channel, &p.id) {
        presence::typing(&ws, &channel, &p.id);
    }
    Json(json!({ "ok": true })).into_response()
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
async fn direct(State(k): K, h: HeaderMap, Path(ws): Path<String>, Json(b): Json<NewDirect>) -> Reply {
    let (p, role) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    let Some(w) = workspace(&k, &ws) else { return err(StatusCode::NOT_FOUND, "no such workspace") };
    let mut people: Vec<String> = b.people.into_iter().filter(|x| x != &p.id).collect();
    people.push(p.id.clone());
    people.sort();
    people.dedup();
    if !people.iter().all(|x| w.members.contains_key(x)) {
        return err(StatusCode::BAD_REQUEST, "a direct message is between members");
    }
    if b.agent && role == Role::Guest {
        return err(StatusCode::FORBIDDEN, "a guest can read and chat, and cannot have the agent act");
    }
    let found = Direct { people: people.clone(), agent: b.agent };
    if let Some((id, _)) = w.chat.direct.iter().find(|(_, d)| **d == found) {
        return Json(json!({ "id": id })).into_response();
    }
    let id = new_id();
    ok_or_return!(send(&k, &p, Change::Channel { workspace: ws.clone(), channel: id.clone(), name: String::new() }).await);
    ok_or_return!(send(&k, &p, Change::Chat { workspace: ws, chat: Chat::Direct { channel: id.clone(), people, agent: b.agent } }).await);
    Json(json!({ "id": id })).into_response()
}

/// Mentions of this person, replies in threads they took part in, and their
/// requests waiting on an answer from them.
async fn activity(State(k): K, h: HeaderMap, Path(ws): Path<String>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    let me = name_of(&k, &p.id);
    let out = k.read(|s| {
        let Some(w) = s.team.workspace(&ws) else { return json!({}) };
        let (mut mentions, mut replies) = (vec![], vec![]);
        for (cid, ch) in w.channels.iter().filter(|(id, c)| w.sees(id, &p.id) && w.in_project(c.project.as_deref(), &p.id)) {
            let mine: Vec<&str> = ch.messages.iter().filter(|m| m.sender == p.id).map(|m| m.thread.as_deref().unwrap_or(&m.id)).collect();
            for m in ch.messages.iter().filter(|m| m.sender != p.id && !w.chat.unsent.contains(&m.id) && w.kept_for(cid, &p.id, m.at)) {
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
    Json(out).into_response()
}
