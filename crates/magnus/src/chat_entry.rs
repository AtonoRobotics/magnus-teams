//! One entry of a conversation as it travels to the app's one chat view
//! (desktop/ui/chat-view.js): a person's message, Magnus's answer, an
//! opener, the tasks a message started, and in a team channel the agent's
//! work and a deleted message's place. The personal chat (`chat.rs`) and the
//! team chat (`team::items`) both build this one type, so both chats send
//! the same shape (decisions/2026-11-13-one-chat-entry.md). What a kind lacks
//! is left out. The kinds the personal chat alone has (cards, results,
//! sub-agents) are still built as JSON where they are made.
use crate::state::Status;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

type At = chrono::DateTime<chrono::Utc>;

/// A file an entry carries. A personal chat file is the person's stored
/// file; a team file is the workspace's, named by its name.
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct EntryFile {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<usize>,
}

impl EntryFile {
    /// A workspace file, known by its name.
    pub fn named(name: &str) -> EntryFile {
        EntryFile { id: name.into(), name: name.into(), ..Default::default() }
    }
}

/// One task as the chat view lists it under the request that started it:
/// the personal chat's `tasks` entry rows, and a team request's work.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TaskRow {
    pub task_id: String,
    pub title: String,
    /// What was asked.
    pub text: String,
    pub status: Status,
    /// Why it waits, when suspended.
    pub waiting_on: Option<String>,
    pub waiting_item: Option<String>,
    /// What it is doing now, in a line.
    pub status_line: Option<String>,
    // A team request's own: whom it is for, how far it is, and where its
    // work and answer show.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requester: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<usize>,
    /// The tool it called last.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doing: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started: Option<At>,
}

/// What the viewer may do with an entry: the one rule the app draws its
/// message controls from and the routes keep. Copy, branch, export and read
/// aloud are anyone's who sees the entry.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct May {
    pub edit: bool,
    pub retry: bool,
    pub feedback: bool,
    pub share: bool,
    pub save: bool,
    pub task: bool,
    pub pin: bool,
    pub delete: bool,
}

impl May {
    /// In a person's own chat they may do everything to every entry but
    /// pin: edit their messages, retry Magnus's turns, and judge what
    /// Magnus said or did.
    pub fn personal(kind: &str) -> May {
        let said = matches!(kind, "answer" | "turn" | "result" | "tasks" | "grant_card" | "suggestion_card" | "schedule_card" | "connector_card" | "opener");
        May { edit: kind == "message", retry: matches!(kind, "answer" | "turn"), feedback: said, share: true, save: true, task: true, pin: false, delete: true }
    }
}

/// One entry of a conversation.
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct ChatEntry {
    pub id: String,
    /// `message`, `answer`, `opener`, `tasks`; in a team channel also
    /// `work` (the agent's work with no words yet) and `deleted`.
    pub kind: &'static str,
    pub text: String,
    pub at: Option<At>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<EntryFile>>,

    // An opener: the app Magnus opened the conversation about.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_title: Option<String>,

    // An answer: its deliverable, its words to be read aloud, the model it
    // ran on and the person's verdict.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spoken: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Value>,

    // The tasks one message started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<Vec<TaskRow>>,

    // A team channel's: who said it and for whom, its thread, and what the
    // members did to it.
    /// A person, or the agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender: Option<String>,
    /// Whom the agent wrote it for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requester: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edited: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    /// Emoji, then who.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reactions: Option<BTreeMap<String, BTreeSet<String>>>,
    /// Deleted for everyone: only its place is left.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted: Option<bool>,
    /// The request an answer answers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// An answer only its requester may read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private: Option<bool>,
    /// The work the message asked for, while it shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work: Option<TaskRow>,
    /// How many replies hang from it, in the channel view.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replies: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_reply: Option<At>,
    /// What the viewer may do with it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub may: Option<May>,
}

impl ChatEntry {
    /// The entry as JSON, for the places that still add to it.
    pub fn value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In their own chat a person edits their messages, retries Magnus's
    /// turns, judges what Magnus said or did, and pins nothing.
    #[test]
    fn a_person_may_do_everything_in_their_own_chat_but_pin() {
        let mine = May::personal("message");
        assert!(mine.edit && !mine.retry && !mine.feedback && mine.delete && mine.share && !mine.pin);
        let answer = May::personal("answer");
        assert!(!answer.edit && answer.retry && answer.feedback && answer.save && answer.task);
        let result = May::personal("result");
        assert!(!result.retry && result.feedback);
    }
}
