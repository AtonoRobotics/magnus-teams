//! What the app tells a person about as it happens, in every workspace they
//! are in, so it can pop up wherever they are in the app and in the
//! computer's own notifications: a message to them, a mention, a message in
//! a channel they follow in full, and the team agent's answer to their
//! request. Each channel's setting (all, mentions or off) decides, as the
//! spec's per-channel notifications say; their own messages never count.
use super::chat::{mentions, Notify};
use super::chat_routes::name_of;
use super::conversation::{AGENT_ID, AGENT_NAME};
use super::routes::{Reply, K};
use crate::api::who;
use crate::kernel::Kernel;
use crate::state::State as Shared;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

type At = chrono::DateTime<chrono::Utc>;

/// The most one look returns: the newest, when more came at once.
const MAX_NOTICES: usize = 20;
/// How much of a message the pop-up shows.
const PREVIEW_CHARS: usize = 140;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/team/notices", get(notices))
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// In a direct conversation with them.
    Direct,
    Mention,
    /// In a channel they follow in full.
    Message,
    /// The team agent finished what they asked.
    Answer,
}

#[derive(Serialize, Clone, Debug)]
pub struct Notice {
    pub id: String,
    pub kind: Kind,
    pub workspace: String,
    pub workspace_name: String,
    pub channel: String,
    pub channel_name: String,
    /// The other people of a direct conversation, or the agent's.
    pub direct: Option<Vec<String>>,
    pub thread: Option<String>,
    pub sender: String,
    pub at: At,
    pub preview: String,
}

fn preview(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim();
    let mut out: String = line.chars().take(PREVIEW_CHARS).collect();
    if line.chars().count() > PREVIEW_CHARS {
        out.push('…');
    }
    out
}

/// Everything after `after` to tell `person` (called `name`) about, oldest
/// first, at most the newest [`MAX_NOTICES`].
pub fn collect(s: &Shared, person: &str, name: &str, after: At) -> Vec<Notice> {
    let mut out = vec![];
    for w in s.team.of(person) {
        for (cid, ch) in w.channels.iter().filter(|(id, c)| w.sees(id, person) && w.in_project(c.project.as_deref(), person)) {
            let level = w.notify_level(cid, person);
            if level == Notify::Off {
                continue;
            }
            let direct = w.chat.direct.get(cid);
            let others = direct.map(|d| if d.agent { vec![AGENT_ID.to_string()] } else { d.people.iter().filter(|p| *p != person).cloned().collect() });
            let read = w.chat.read.get(person).and_then(|c| c.get(cid)).copied();
            let since = Some(after).max(read).max(w.cleared(cid, person));
            let notice = |id: String, kind: Kind, sender: String, thread: Option<String>, at: At, text: &str| Notice {
                id,
                kind,
                workspace: w.id.clone(),
                workspace_name: w.name.clone(),
                channel: cid.clone(),
                channel_name: ch.name.clone(),
                direct: others.clone(),
                thread,
                sender,
                at,
                preview: preview(text),
            };
            for m in ch.messages.iter().filter(|m| since.is_none_or(|t| m.at > t) && m.sender != person && !m.text.is_empty() && !w.chat.unsent.contains(&m.id)) {
                let kind = if mentions(&m.text, person, name) {
                    Kind::Mention
                } else if level == Notify::Mentions {
                    continue;
                } else if direct.is_some() {
                    Kind::Direct
                } else {
                    Kind::Message
                };
                let sender = if m.sender.starts_with("agent:") { AGENT_ID.to_string() } else { m.sender.clone() };
                out.push(notice(m.id.clone(), kind, sender, m.thread.clone(), m.at, &m.text));
            }
            for r in s.team.runs_in(&w.id, cid).into_iter().filter(|r| r.requester == person) {
                let Some(task) = s.tasks.get(&r.task) else { continue };
                let Some(run) = s.runs.get(&task.run_id) else { continue };
                let Some(at) = run.ended_at.filter(|at| *at > after && run.status.terminal()) else { continue };
                let text = match run.status {
                    crate::state::Status::Completed => run.summary.clone().unwrap_or_default(),
                    crate::state::Status::Failed => format!("I could not finish this: {}", run.reason.as_deref().unwrap_or("it failed")),
                    _ => continue,
                };
                let thread = super::conversation::placement(
                    r.thread.as_deref(),
                    w.chat.asked.get(&r.task).map(String::as_str),
                    super::conversation::is_long(task.created_at, at, run.replies.len()),
                );
                out.push(notice(format!("agent:{}", r.task), Kind::Answer, AGENT_ID.to_string(), thread, at, &text));
            }
        }
    }
    out.sort_by_key(|n| n.at);
    let extra = out.len().saturating_sub(MAX_NOTICES);
    out.drain(..extra);
    out
}

#[derive(Deserialize)]
struct After {
    /// The `now` of the last look. Without one, nothing is old news yet:
    /// the answer is only the time to ask from next.
    #[serde(default)]
    after: Option<At>,
}

async fn notices(State(k): K, h: HeaderMap, Query(q): Query<After>) -> Reply {
    let p = ok_or_return!(who(&k, &h).await.map_err(Box::new));
    let now = chrono::Utc::now();
    let Some(after) = q.after else { return Json(json!({ "now": now, "notices": [], "names": {} })).into_response() };
    let me = name_of(&k, &p.id);
    let found = k.read(|s| collect(s, &p.id, &me, after));
    let mut names = serde_json::Map::new();
    for id in found.iter().flat_map(|n| std::iter::once(&n.sender).chain(n.direct.iter().flatten())) {
        let name = if id == AGENT_ID { AGENT_NAME.to_string() } else { name_of(&k, id) };
        names.insert(id.clone(), json!(name));
    }
    Json(json!({ "now": now, "notices": found, "names": names })).into_response()
}
