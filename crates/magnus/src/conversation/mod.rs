//! One conversation, for one person or many
//! (decisions/2026-11-14-one-conversation.md). The personal chat and a team
//! channel keep their own stores (decisions/2026-11-13-one-chat-store-not-yet.md):
//! a person's words in their own store, a team's in the workspace ledger.
//! Above the store they are one thing, a `Conversation`: its entries as the
//! viewer has them, one entry found by id, what the viewer may do with it,
//! who said it, and what the conversation has beside its entries (`Caps`).
//!
//! What both do alike is written here once: an entry the viewer does not
//! have is a 404 that does not say whether it exists, an action `may` does
//! not allow is a 403, and an entry exports as the same Markdown file.
//!
//! What each action means differs, and stays with its conversation on
//! purpose (`personal`, `channel`):
//! - delete: a person's message goes to their trash and can be restored; a
//!   team message is gone for everyone and cannot be brought back;
//! - edit: a person edits and resends, and what came after leaves the
//!   thread; a team message is edited in place and marked edited;
//! - share: a person shares with anyone on the server, through their inbox,
//!   within the receiver's rights and with the entry's taint; a team
//!   message is quoted to another member of the workspace only;
//! - branch: a person's thread branches on the server; a team message is
//!   quoted into a new private chat in the app, unsent, so team words reach
//!   the person's own agent only when they send them (inv8);
//! - save: to the person's Files, or to the workspace's files;
//! - reactions: a person's great or annoying is a private verdict Magnus
//!   learns from; a team's emoji are public to the channel.
//!
//! Nothing here moves words between the two: no personal answer is posted
//! into a channel and no team message is shared to the server's directory.
//! Either would be a new promise about whose words go where.
use crate::api_error::{ApiError, ApiResult};
use crate::chat_entry::May;
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

pub mod channel;
pub mod personal;
#[cfg(test)]
mod tests;

type At = chrono::DateTime<chrono::Utc>;

/// What an entry the viewer does not have is called, in every refusal.
pub const NO_SUCH_MESSAGE: &str = "no such message";

/// What a conversation has beside its entries. The app draws from these
/// instead of guessing from what it was handed (`caps` on both views).
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Caps {
    /// Several people write in it: each message names who sent it, and
    /// mentions are marked.
    pub several: bool,
    /// Members react with emoji, which everyone there sees.
    pub reactions: bool,
    /// Messages are pinned for everyone there.
    pub pins: bool,
    /// A message has a thread of replies.
    pub threads: bool,
    /// The viewer may ask their own Magnus privately, beside the others.
    pub whisper: bool,
    /// Who is typing shows.
    pub typing: bool,
}

/// One conversation as one viewer has it.
pub trait Conversation {
    type Entry;

    /// Every entry the viewer has, in order.
    fn entries(&self) -> Vec<Self::Entry>;
    /// The entry `id`, if the viewer has it.
    fn find(&self, id: &str) -> Option<Self::Entry>;
    /// What the viewer may do with it; nothing for an entry no one acts on.
    fn may(&self, e: &Self::Entry) -> Option<May>;
    /// Who said it, as a person reading it calls them.
    fn speaker_name(&self, e: &Self::Entry) -> String;
    /// What it says, as text.
    fn text(&self, e: &Self::Entry) -> String;
    /// When it was said.
    fn said_at(&self, e: &Self::Entry) -> Option<At>;
    fn caps(&self) -> Caps;

    /// The entry `id`, or a 404 that does not say whether it exists.
    fn entry(&self, id: &str) -> ApiResult<Self::Entry> {
        self.find(id).ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, NO_SUCH_MESSAGE))
    }

    /// The entry `id` if the viewer has it (else 404) and `allowed` lets
    /// them act on it (else 403 saying `why`).
    fn act(&self, id: &str, allowed: impl Fn(&May) -> bool, why: &str) -> ApiResult<Self::Entry> {
        let e = self.entry(id)?;
        if !self.may(&e).as_ref().is_some_and(allowed) {
            return Err(ApiError::new(StatusCode::FORBIDDEN, why));
        }
        Ok(e)
    }

    /// The entry as a Markdown file to save on this computer.
    fn export(&self, id: &str) -> ApiResult<Response> {
        let e = self.entry(id)?;
        Ok(markdown_file(&self.speaker_name(&e), self.said_at(&e), &self.text(&e)))
    }
}

/// One entry as a Markdown file, the same from either conversation.
pub fn markdown_file(who: &str, at: Option<At>, text: &str) -> Response {
    let when = at.map(crate::message_file::at_text).unwrap_or_default();
    let body = crate::message_file::markdown(who, &when, text);
    let name = crate::message_file::file_name(text);
    ([(CONTENT_TYPE, "text/markdown; charset=utf-8".to_string()), (CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))], body).into_response()
}
