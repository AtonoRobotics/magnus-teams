//! What each person keeps of the team chat: deleting a message they sent
//! (gone for everyone, its text out of the log and the backups), and putting
//! a conversation away or deleting it from their own list, which leaves
//! everyone else's as it was.
use super::chat::Chat;
use super::member_view::MemberView;
use super::routes::{has_channel, member, send, Reply, K};
use super::{Change, Message, Role, Workspace};
use crate::api::err;
use crate::config::Person;
use crate::events::Ev;
use crate::kernel::Kernel;
use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

type At = chrono::DateTime<chrono::Utc>;

/// What a removal of a deleted message is called: this, then the message.
pub const MESSAGE: &str = "message:";

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/workspaces/{ws}/channels/{channel}/archive", post(archive)).route("/v1/workspaces/{ws}/channels/{channel}/clear", post(clear))
}

impl Workspace {
    /// Up to when this person deleted the conversation for themselves.
    pub fn cleared(&self, channel: &str, person: &str) -> Option<At> {
        self.chat.cleared.get(person).and_then(|c| c.get(channel)).copied()
    }

    /// Whether this person put the conversation away and nobody else has
    /// written in it since.
    pub fn archived(&self, channel: &str, person: &str) -> bool {
        let Some(at) = self.chat.archived.get(person).and_then(|c| c.get(channel)) else { return false };
        !self.channels.get(channel).is_some_and(|ch| ch.messages.iter().any(|m| m.sender != person && m.at > *at && !self.chat.unsent.contains(&m.id)))
    }

    /// Whether a conversation this person deleted has had nothing new since,
    /// so it stays off their list.
    pub fn deleted_for(&self, channel: &str, person: &str) -> bool {
        let Some(at) = self.cleared(channel, person) else { return false };
        !self.channels.get(channel).is_some_and(|ch| ch.messages.iter().any(|m| m.at > at && !self.chat.unsent.contains(&m.id)))
    }

    /// Whether this person still has something at `at` in the channel: it
    /// came after anything they deleted for themselves.
    pub fn kept_for(&self, channel: &str, person: &str, at: At) -> bool {
        self.cleared(channel, person).is_none_or(|c| at > c)
    }
}

impl Kernel {
    /// Delete a message for everyone: the place it stood stays, its text
    /// goes, from the ledger at once and from the log and the backups.
    pub(super) async fn unsend(&self, actor: &str, ws: &str, channel: &str, m: &Message) -> Result<()> {
        let gone = Chat::Unsent { channel: channel.to_string(), id: m.id.clone(), sender: m.sender.clone(), thread: m.thread.clone(), at: m.at };
        self.emit(actor, Ev::Team(Change::Chat { workspace: ws.to_string(), chat: gone })).await?;
        let seqs = self.read(|s| s.team.message_seqs(&m.id).to_vec());
        if seqs.is_empty() {
            return Ok(());
        }
        self.emit(actor, Ev::Removed { item: format!("{MESSAGE}{}", m.id), seqs: seqs.clone() }).await?;
        self.log.delete_content(&seqs).await?;
        if let Err(e) = self.scrub_backups().await {
            tracing::error!("taking a deleted message out of the backups: {e:#}");
        }
        Ok(())
    }
}

/// Its sender deletes a message, as only they edit it; and the workspace's
/// owners and admins moderate its channels: "The teams admin can moderate
/// and delete it only from teams" (the owner, 2026-11-11). A direct
/// conversation is its people's alone.
pub(super) async fn unsend(State(k): K, h: HeaderMap, Path((ws, channel, id)): Path<(String, String, String)>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    // The agent's answer is drawn from its request's result, not kept as a
    // message: it is found by its request (owner, 2026-11-13: deleting one
    // said "no such message").
    let now = chrono::Utc::now();
    // Who may: who wrote it, whom the agent wrote it for, and the
    // workspace's admins outside a direct conversation (`may`).
    ok_or_return!(super::may::check(&k, &ws, &channel, &id, &p.id, |m| m.delete, "only who wrote a message, or the workspace's admins, delete it"));
    let Some(m) = k.read(|s| MemberView::of(s, &ws, &p.id).and_then(|v| v.message(&channel, &id, now).map(|m| m.as_message()))) else {
        return err(StatusCode::NOT_FOUND, "no such message");
    };
    if let Err(e) = k.unsend(&actor(&p), &ws, &channel, &m).await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"));
    }
    Json(json!({ "ok": true })).into_response()
}

fn actor(p: &Person) -> String {
    format!("person:{}", p.id)
}

#[derive(Deserialize)]
struct Archive {
    on: bool,
}

/// Put a conversation away, or bring it back. Only the caller's list changes.
async fn archive(State(k): K, h: HeaderMap, Path((ws, channel)): Path<(String, String)>, Json(b): Json<Archive>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    if !has_channel(&k, &ws, &channel, &p.id) {
        return err(StatusCode::NOT_FOUND, "no such channel");
    }
    let chat = Chat::Archived { channel, person: p.id.clone(), on: b.on, at: chrono::Utc::now() };
    ok_or_return!(send(&k, &p, Change::Chat { workspace: ws, chat }).await);
    Json(json!({ "ok": true })).into_response()
}

/// Delete a direct conversation from the caller's own list and view. The
/// other people in it keep theirs; it comes back, from then on only, when
/// someone writes in it again. A channel is the workspace's, so it is put
/// away instead.
async fn clear(State(k): K, h: HeaderMap, Path((ws, channel)): Path<(String, String)>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    if !has_channel(&k, &ws, &channel, &p.id) {
        return err(StatusCode::NOT_FOUND, "no such channel");
    }
    if !k.read(|s| s.team.workspace(&ws).is_some_and(|w| w.chat.direct.contains_key(&channel))) {
        return err(StatusCode::BAD_REQUEST, "only a direct conversation is deleted for one person; put a channel away instead");
    }
    let chat = Chat::Cleared { channel, person: p.id.clone(), at: chrono::Utc::now() };
    ok_or_return!(send(&k, &p, Change::Chat { workspace: ws, chat }).await);
    Json(json!({ "ok": true })).into_response()
}
