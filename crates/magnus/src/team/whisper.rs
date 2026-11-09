//! A person's own agent in a thread. It sees the thread as data, and what it
//! says reaches only its owner until they post it.
use super::routes::{member, new_id, send, Reply, K};
use super::{Change, Message, Role};
use crate::api::err;
use crate::events::Ev;
use crate::kernel::{Kernel, NewRun};
use crate::state::Run;
use crate::tools::Label;
use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
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
    Router::new().route("/v1/workspaces/{ws}/channels/{channel}/whispers", get(mine)).route("/v1/workspaces/{ws}/whispers/{task}/share", post(share))
}

impl Kernel {
    /// A personal run, its person's own in every way, that also has the thread.
    pub(super) async fn whisper(self: &Arc<Self>, workspace: &str, channel: &str, thread: Option<String>, person: &str, text: String) -> Result<(String, String)> {
        let all: Vec<Message> = self.read(|s| s.team.workspace(workspace).and_then(|w| w.channels.get(channel)).map(|c| c.messages.clone())).unwrap_or_default();
        let here: Vec<&Message> = all.iter().filter(|m| thread.is_none() || m.thread == thread || Some(&m.id) == thread.as_ref()).collect();
        let shown = &here[here.len().saturating_sub(HISTORY_MESSAGES)..];
        let history = shown.iter().map(|m| format!("{}: {}", m.sender, m.text)).collect::<Vec<_>>().join("\n");
        let others = shown.iter().any(|m| m.sender != person);
        let actor = format!("person:{person}");
        let (task, run) = self.create_run(&actor, person, text, vec![], NewRun { label: others.then_some(Label::UNTRUSTED), ..Default::default() }).await?;
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

/// Only the owner's own whispers, whatever their role here.
async fn mine(State(k): K, h: HeaderMap, Path((ws, channel)): Path<(String, String)>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
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
    Json(view).into_response()
}

/// The owner's explicit share: the one way a whisper reaches the thread.
async fn share(State(k): K, h: HeaderMap, Path((ws, task)): Path<(String, String)>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    let found = k.read(|s| s.team.whisper_of(&task).filter(|w| w.workspace == ws && w.owner == p.id).cloned());
    let Some(w) = found else { return err(StatusCode::NOT_FOUND, "you have no such whisper") };
    if w.shared {
        return err(StatusCode::CONFLICT, "already posted");
    }
    let text = k.read(|s| s.tasks.get(&task).and_then(|t| s.runs.get(&t.run_id)).filter(|r| r.status.terminal()).and_then(|r| r.summary.clone()));
    let Some(text) = text else { return err(StatusCode::CONFLICT, "it has not finished") };
    let message = Message { id: new_id(), sender: p.id.clone(), text, thread: w.thread.clone(), at: chrono::Utc::now() };
    let id = message.id.clone();
    ok_or_return!(send(&k, &p, Change::Message { workspace: ws.clone(), channel: w.channel.clone(), message }).await);
    ok_or_return!(send(&k, &p, Change::WhisperShared { workspace: ws, task }).await);
    Json(json!({ "id": id })).into_response()
}
