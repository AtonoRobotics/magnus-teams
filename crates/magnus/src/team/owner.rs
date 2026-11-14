//! Whose a team event's content is, so erasing a person takes it with them
//! (owner, 2026-11-13; decisions/2026-11-13-erasure-takes-team-words-and-the-computer.md).
//! `log::CONTENT_OWNERS` says the same in SQL, for rows the log does not
//! yet say of; `owner_tests` holds the two to each other.
//!
//! - A message and each edit of it are its sender's. A message the team
//!   agent posted on someone's request (`agent:<them>`) is that request's
//!   work, so it is theirs, as the request's answer already is.
//! - A reaction, a read mark, a notification setting, an archive or a
//!   clear is the person's who made it.
//! - A request's working notes, the channel as a request was given it, and
//!   a whisper are the requester's.
//! - Anything else says whose it is in its own `person`, or is the
//!   workspace's.
use super::chat::Chat;
use super::{Change, Ledger};

/// The person a sender stands for: themselves, or the one the team agent
/// posted for.
pub fn person_of(sender: &str) -> &str {
    sender.strip_prefix("agent:").unwrap_or(sender)
}

impl Ledger {
    /// Whose this change's content is, by the rule above.
    pub fn content_owner(&self, c: &Change) -> Option<String> {
        let who = match c {
            Change::Message { message, .. } => person_of(&message.sender).to_string(),
            Change::Chat { workspace, chat: Chat::Edited { channel, id, .. } } => self.sender_of(workspace, channel, id).map(|s| person_of(s).to_string())?,
            Change::Chat { chat, .. } => serde_json::to_value(chat).ok()?["person"].as_str()?.to_string(),
            Change::Note { note, .. } => person_of(&note.by).to_string(),
            Change::Run { requester, .. } => requester.clone(),
            Change::Whisper { owner, .. } => owner.clone(),
            _ => serde_json::to_value(c).ok()?["person"].as_str()?.to_string(),
        };
        Some(who).filter(|w| !w.is_empty())
    }

    /// Who sent this message.
    pub fn sender_of(&self, workspace: &str, channel: &str, id: &str) -> Option<&str> {
        let ch = self.workspaces.get(workspace)?.channels.get(channel)?;
        ch.messages.iter().find(|m| m.id == id).map(|m| m.sender.as_str())
    }
}
