//! A team channel, or one of its threads, as one member sees it: what they
//! have of it is what their member view gives (`team::member_view`), and
//! what they may do with an entry is the team's one rule (`team::may`).
//! Its actions' own meanings are the team's routes: delete for everyone,
//! edit in place, share quoted to a member, save to the workspace's files,
//! public emoji, and branch in the app, quoted and unsent.
use super::{At, Caps, Conversation};
use crate::api_error::{ApiError, ApiResult};
use crate::chat_entry::May;
use crate::kernel::Kernel;
use crate::state::State;
use crate::team::conversation::{AGENT_ID, AGENT_NAME};
use crate::team::items::{self, TeamItem};
use crate::team::member_view::{author, MemberView, Runs};
use crate::team::Role;
use axum::http::StatusCode;
use std::collections::{BTreeMap, BTreeSet};

/// People's names by id, read before the member view is: a name is read
/// from the directory, which may not be read while the state is.
pub type Names = BTreeMap<String, String>;

/// The name of everyone in workspace `ws` and everyone who wrote there
/// (one who left among them), and the agent's.
pub fn names(k: &Kernel, ws: &str) -> Names {
    let ids: BTreeSet<String> = k.read(|s| {
        let Some(w) = s.team.workspace(ws) else { return BTreeSet::new() };
        let senders = w.channels.values().flat_map(|c| c.messages.iter()).map(|m| author(&m.sender).0.to_string());
        w.members.keys().cloned().chain(senders).collect()
    });
    let mut out: Names = ids.into_iter().map(|id| (k.people().name(&id), id)).map(|(name, id)| (id, name)).collect();
    out.insert(AGENT_ID.into(), AGENT_NAME.into());
    out
}

/// A channel (`thread` None) or one of its threads, for one member, at `now`.
pub struct Channel<'a, R: Runs> {
    pub view: MemberView<'a, R>,
    pub channel: &'a str,
    pub thread: Option<&'a str>,
    pub now: At,
    pub names: &'a Names,
}

/// `f` on channel `channel` of workspace `ws` as `person` has it now: a
/// 404 when they are not in the workspace.
pub fn read<T>(k: &Kernel, ws: &str, person: &str, channel: &str, names: &Names, f: impl FnOnce(&Channel<'_, State>) -> ApiResult<T>) -> ApiResult<T> {
    let now = chrono::Utc::now();
    k.read(|s| match MemberView::of(s, ws, person) {
        Some(view) => f(&Channel { view, channel, thread: None, now, names }),
        None => Err(ApiError::new(StatusCode::NOT_FOUND, super::NO_SUCH_MESSAGE)),
    })
}

impl<'a, R: Runs> Channel<'a, R> {
    /// What a channel has for a member with `role`: several people, their
    /// emoji, pins, threads and typing; and a whisper to one's own Magnus,
    /// which a guest, who has no agent act for them, does not have.
    pub fn caps_for(role: Role) -> Caps {
        Caps { several: true, reactions: true, pins: true, threads: true, whisper: role > Role::Guest, typing: true }
    }

    fn role(&self) -> Role {
        self.view.workspace().members.get(self.view.person()).copied().unwrap_or(Role::Guest)
    }
}

impl<R: Runs> Conversation for Channel<'_, R> {
    type Entry = TeamItem;

    /// The channel's timeline; in a thread, its first message and replies.
    fn entries(&self) -> Vec<TeamItem> {
        let all = items::timeline(&self.view, self.channel, self.now);
        match self.thread {
            None => all,
            Some(t) => all.into_iter().filter(|i| i.id == t || i.thread.as_deref() == Some(t)).collect(),
        }
    }

    /// A message or answer the member has, not deleted, with what they may do.
    fn find(&self, id: &str) -> Option<TeamItem> {
        crate::team::may::entry(&self.view, self.channel, id, self.now)
    }

    fn may(&self, e: &TeamItem) -> Option<May> {
        crate::team::may::may(&self.view, self.channel, e)
    }

    fn speaker_name(&self, e: &TeamItem) -> String {
        let id = e.sender.as_deref().unwrap_or_default();
        self.names.get(id).cloned().unwrap_or_else(|| id.to_string())
    }

    /// Its words as they were written, its files' lines among them.
    fn text(&self, e: &TeamItem) -> String {
        e.text.clone()
    }

    fn said_at(&self, e: &TeamItem) -> Option<At> {
        e.at
    }

    fn caps(&self) -> Caps {
        Self::caps_for(self.role())
    }
}
