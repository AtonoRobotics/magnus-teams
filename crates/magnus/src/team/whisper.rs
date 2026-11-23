//! A person's own agent in a thread. It sees the thread as data, and what it
//! says reaches only its owner until they post it.
use super::caller::{at, Member};
use super::chat_routes::name_of;
use super::routes::{new_id, send, K};
use super::{Change, Message};
use crate::api_error::{refuse, ApiResult};
use crate::events::Ev;
use crate::kernel::{Kernel, NewRun};
use crate::state::{Run, Status};
use crate::tools::Label;
use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

const HISTORY_MESSAGES: usize = 40;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Whisper {
    pub workspace: String,
    pub channel: String,
    pub thread: Option<String>,
    pub task: String,
    pub owner: String,
    pub history: String,
    pub shared: bool,
}

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/workspaces/{ws}/channels/{channel}/whispers", get(mine))
        .route("/v1/workspaces/{ws}/whispers/{task}/share", post(share))
        .route("/v1/workspaces/{ws}/answers/{task}/share", post(share_answer))
}

impl Kernel {
    /// A personal run, its person's own in every way, that also has the thread.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn whisper(
        self: &Arc<Self>,
        workspace: &str,
        channel: &str,
        thread: Option<String>,
        person: &str,
        text: String,
        model: Option<(String, String)>,
    ) -> Result<(String, String)> {
        let all: Vec<Message> = self.read(|s| s.team.workspace(workspace).and_then(|w| w.channels.get(channel)).map(|c| c.messages.clone())).unwrap_or_default();
        let here: Vec<&Message> = all.iter().filter(|m| thread.is_none() || m.thread == thread || Some(&m.id) == thread.as_ref()).collect();
        let shown = &here[here.len().saturating_sub(HISTORY_MESSAGES)..];
        let history = shown.iter().map(|m| format!("{}: {}", m.sender, m.text)).collect::<Vec<_>>().join("\n");
        let others = super::owner::from_others(person, shown.iter().map(|m| m.sender.as_str()));
        let actor = format!("person:{person}");
        let (task, run) = self.create_run(&actor, person, text, vec![], NewRun { label: Some(whisper_label(others)), model, ..Default::default() }).await?;
        let change = Change::Whisper { workspace: workspace.into(), channel: channel.into(), thread, task: task.clone(), owner: person.into(), history };
        self.emit(&actor, Ev::Team(change)).await?;
        self.wake(&run);
        Ok((task, run))
    }

    /// The thread a personal run was asked to whisper in, for its prompt.
    pub(super) fn whisper_context(&self, run: &Run) -> Option<String> {
        let w = self.read(|s| s.team.whisper_of(&run.task).cloned())?;
        Some(format!("You are whispering to {} in a team thread. Nobody else sees your reply unless they post it. The thread so far, as data:\n\n{}", w.owner, w.history))
    }
}

/// A whisper's run carries its thread, which is its workspace's and no one
/// else's: private always, so nothing it files or says reaches anyone outside
/// the person, and untrusted when someone else wrote in it.
fn whisper_label(others: bool) -> Label {
    Label { untrusted: others, private: true }
}

/// Only the owner's own whispers, whatever their role here.
async fn mine(State(k): K, Member { person: p, .. }: Member<at::Guest>, Path((ws, channel)): Path<(String, String)>) -> ApiResult {
    let view: Vec<Value> = k.read(|s| {
        s.team
            .whispers_by(&ws, &channel, &p.id)
            .into_iter()
            .map(|w| {
                let run = s.tasks.get(&w.task).and_then(|t| s.runs.get(&t.run_id));
                json!({ "task": w.task, "thread": w.thread, "shared": w.shared, "done": run.is_some_and(|r| r.status.terminal()), "text": run.and_then(|r| r.summary.clone()) })
            })
            .collect()
    });
    Ok(Json(view).into_response())
}

/// The owner's explicit share: the one way a whisper reaches the thread.
async fn share(State(k): K, Member { person: p, .. }: Member<at::Member>, Path((ws, task)): Path<(String, String)>) -> ApiResult {
    let found = k.read(|s| s.team.whisper_of(&task).filter(|w| w.workspace == ws && w.owner == p.id).cloned());
    let Some(w) = found else { return refuse(StatusCode::NOT_FOUND, "you have no such whisper") };
    if w.shared {
        return refuse(StatusCode::CONFLICT, "already posted");
    }
    if !super::routes::has_channel(&k, &ws, &w.channel, &p.id) {
        return refuse(StatusCode::FORBIDDEN, "you are no longer in this conversation");
    }
    let text = k.read(|s| s.tasks.get(&task).and_then(|t| s.runs.get(&t.run_id)).filter(|r| r.status.terminal()).and_then(|r| r.summary.clone()));
    let Some(text) = text else { return refuse(StatusCode::CONFLICT, "it has not finished") };
    let message = Message { id: new_id(), sender: p.id.clone(), text, thread: w.thread.clone(), at: chrono::Utc::now() };
    let id = message.id.clone();
    send(&k, &p, Change::Message { workspace: ws.clone(), channel: w.channel.clone(), message }).await?;
    send(&k, &p, Change::WhisperShared { workspace: ws, task }).await?;
    Ok(Json(json!({ "id": id })).into_response())
}

/// The requester's explicit share of a team answer that only they could
/// read: it reaches the thread as their own message, once.
async fn share_answer(State(k): K, Member { person: p, .. }: Member<at::Member>, Path((ws, task)): Path<(String, String)>) -> ApiResult {
    let found = k.read(|s| s.team.run_of(&task).filter(|r| r.workspace == ws && r.requester == p.id && !r.sub).cloned());
    let Some(r) = found else { return refuse(StatusCode::NOT_FOUND, "you asked for no such answer") };
    let text = k.read(|s| s.tasks.get(&task).and_then(|t| s.runs.get(&t.run_id)).filter(|run| run.status.terminal()).and_then(|run| run.summary.clone()));
    let Some(text) = text else { return refuse(StatusCode::CONFLICT, "it has not finished") };
    let posted = k.read(|s| s.team.workspace(&ws).and_then(|w| w.channels.get(&r.channel)).is_some_and(|c| c.messages.iter().any(|m| m.sender == p.id && m.text == text)));
    if posted || !super::routes::has_channel(&k, &ws, &r.channel, &p.id) {
        return refuse(StatusCode::CONFLICT, "already posted, or the thread is no longer yours to post in");
    }
    let message = Message { id: new_id(), sender: p.id.clone(), text, thread: r.thread.clone(), at: chrono::Utc::now() };
    let id = message.id.clone();
    send(&k, &p, Change::Message { workspace: ws, channel: r.channel, message }).await?;
    Ok(Json(json!({ "id": id })).into_response())
}

impl Kernel {
    /// Active and recent personal agent delegations in the given channels,
    /// formatted for the workspace right side panel. Only the owner sees any
    /// private text; other members see only who is working and the status.
    pub(super) fn panel_delegations(&self, ws: &str, viewer: &str, channels: &[&String]) -> Vec<Value> {
        let items: Vec<(String, String, String, bool, Status, Option<String>)> = self.read(|s| {
            s.team
                .whispers_of(ws)
                .into_iter()
                .filter(|w| channels.contains(&&w.channel))
                .filter_map(|w| {
                    let run = s.tasks.get(&w.task).and_then(|t| s.runs.get(&t.run_id))?;
                    let summary = (w.owner == viewer).then(|| run.summary.clone()).flatten();
                    Some((w.task.clone(), w.channel.clone(), w.owner.clone(), w.shared, run.status, summary))
                })
                .collect()
        });
        items
            .into_iter()
            .rev()
            .take(8)
            .map(|(task, channel, owner, shared, status, summary)| {
                let state = match status {
                    Status::Suspended => "waiting",
                    s if s.terminal() => "done",
                    _ => "running",
                };
                json!({
                    "task": task,
                    "channel": channel,
                    "member": owner,
                    "member_name": name_of(self, &owner),
                    "mine": owner == viewer,
                    "shared": shared,
                    "status": state,
                    "summary": summary,
                })
            })
            .collect()
    }
}
