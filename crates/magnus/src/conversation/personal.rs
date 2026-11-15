//! A person's own conversation with Magnus: one thread of theirs. Who has
//! it is decided where its entries are read (`Kernel::thread_entries`, from
//! the turns and branch whose person is them): to anyone else the thread is
//! not there. Its actions' own meanings are in `chat_actions`: delete to the
//! trash, edit and resend, share through the inbox, branch on the server,
//! save to Files, and a private verdict as its reaction.
use super::{At, Caps, Conversation};
use crate::api_error::{ApiError, ApiResult};
use crate::chat_entry::May;
use crate::chat_thread::Entry;
use crate::kernel::Kernel;
use axum::http::StatusCode;

/// One person and Magnus: no one else writes, reacts, pins or types here.
pub const CAPS: Caps = Caps { several: false, reactions: false, pins: false, threads: false, whisper: false, typing: false };

/// The thread as the app is sent it: its entries, and what it has beside
/// them (`caps`), which the app draws from.
pub fn view(thread: &str, entries: Vec<Entry>) -> serde_json::Value {
    serde_json::json!({ "thread": thread, "entries": entries, "caps": CAPS })
}

/// A person's thread as they see it now.
pub struct Personal {
    pub person: String,
    pub thread: String,
    /// The thread's entries, in order.
    pub entries: Vec<Entry>,
}

impl Personal {
    /// The person's thread `thread`, or a 404 when it is not theirs or not there.
    pub async fn open(k: &Kernel, person: &str, thread: &str) -> ApiResult<Personal> {
        let entries = k.thread_entries(person, thread).await?.ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "no such thread"))?;
        Ok(Personal::of(person, thread, entries))
    }

    /// The thread from entries already read for `person`.
    pub fn of(person: &str, thread: &str, entries: Vec<Entry>) -> Personal {
        Personal { person: person.into(), thread: thread.into(), entries }
    }

    /// Where entry `id` is in the thread, or a 404.
    pub fn place(&self, id: &str) -> ApiResult<usize> {
        self.entries.iter().position(|e| e.has_id() && e.id == id).ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, super::NO_SUCH_MESSAGE))
    }
}

impl Conversation for Personal {
    type Entry = Entry;

    fn entries(&self) -> Vec<Entry> {
        self.entries.clone()
    }

    fn find(&self, id: &str) -> Option<Entry> {
        self.place(id).ok().map(|at| self.entries[at].clone())
    }

    /// The person may do everything to an entry of theirs but pin.
    fn may(&self, e: &Entry) -> Option<May> {
        Some(e.may.unwrap_or_else(|| May::personal(e.kind)))
    }

    fn speaker_name(&self, e: &Entry) -> String {
        e.speaker().to_string()
    }

    fn text(&self, e: &Entry) -> String {
        e.text()
    }

    fn said_at(&self, e: &Entry) -> Option<At> {
        e.at
    }

    fn caps(&self) -> Caps {
        CAPS
    }
}
