//! The team chat's items as they travel to the app: a person's message, the
//! agent's live work, and its answer. Defined once here; the app's JSDoc
//! (`TeamMessage`, `TeamWork` in desktop/ui/team-chat.js) is held to them by
//! `desktop/tests/fixtures/team-items.json`.
use super::conversation::AGENT_ID;
use super::member_view::{author, Answer, MemberView, Runs, Work};
use super::{Message, Workspace};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

type At = chrono::DateTime<chrono::Utc>;

/// A request's live card.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TeamWork {
    pub task: String,
    pub requester: String,
    /// working, waiting, done, failed or stopped.
    pub status: &'static str,
    pub steps: usize,
    pub doing: Option<String>,
    pub title: String,
    pub long: bool,
    pub thread: Option<String>,
    pub started: At,
    pub reason: Option<String>,
}

/// One item of a channel: a message, a request's work with no message to
/// sit on, or the agent's answer. What one kind lacks is left out.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TeamItem {
    pub id: String,
    /// A person, or the agent.
    pub sender: String,
    /// Whom the agent wrote it for.
    pub requester: Option<String>,
    pub text: String,
    pub thread: Option<String>,
    pub at: At,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work: Option<TeamWork>,
    /// How many replies hang from it, in the channel view.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replies: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_reply: Option<At>,
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

impl TeamWork {
    pub fn of(w: &Work) -> TeamWork {
        TeamWork {
            task: w.task.clone(),
            requester: w.requester.clone(),
            status: status_word(w.status),
            steps: w.steps,
            doing: w.doing.clone(),
            title: w.title.clone(),
            long: w.long,
            thread: w.thread.clone(),
            started: w.started,
            reason: w.reason.clone(),
        }
    }
}

impl TeamItem {
    fn bare(id: String, sender: String, requester: Option<String>, text: String, thread: Option<String>, at: At) -> TeamItem {
        TeamItem {
            id,
            sender,
            requester,
            text,
            thread,
            at,
            edited: None,
            pinned: None,
            reactions: None,
            deleted: None,
            task: None,
            private: None,
            work: None,
            replies: None,
            last_reply: None,
        }
    }

    /// A message, with its edits, pin, reactions and whether it was deleted.
    pub fn message(w: &Workspace, m: &Message) -> TeamItem {
        let (sender, requester) = author(&m.sender);
        TeamItem {
            edited: Some(w.chat.edited.contains(&m.id)),
            pinned: Some(w.chat.pins.contains(&m.id)),
            reactions: Some(w.chat.reactions.get(&m.id).cloned().unwrap_or_default()),
            deleted: Some(w.chat.unsent.contains(&m.id)),
            ..TeamItem::bare(m.id.clone(), sender.to_string(), requester.map(String::from), m.text.clone(), m.thread.clone(), m.at)
        }
    }

    /// A request's work that no message carries: one the agent began itself.
    pub fn work(w: &Work) -> TeamItem {
        TeamItem {
            work: Some(TeamWork::of(w)),
            ..TeamItem::bare(format!("work:{}", w.task), AGENT_ID.into(), Some(w.requester.clone()), String::new(), w.asked_in.clone(), w.started)
        }
    }

    /// The agent's answer, as the viewer may read it.
    pub fn answer(a: &Answer) -> TeamItem {
        TeamItem {
            task: Some(a.task.clone()),
            private: Some(a.private),
            ..TeamItem::bare(a.id.clone(), AGENT_ID.into(), Some(a.requester.clone()), a.text.clone(), a.thread.clone(), a.at)
        }
    }
}

/// A channel as this person reads it now, in order: its messages with the
/// agent's work cards on the ones that asked, work no message carries, and
/// the agent's answers; each with how many replies hang from it. A deleted
/// message is left only where replies still hang from it.
pub fn timeline<R: Runs>(v: &MemberView<R>, channel: &str, now: At) -> Vec<TeamItem> {
    let w = v.workspace();
    let work = v.work(channel, now);
    let mut items: Vec<TeamItem> = v
        .places(channel)
        .into_iter()
        .map(|m| TeamItem { work: work.iter().find(|x| x.asked.as_deref() == Some(m.id.as_str())).map(TeamWork::of), ..TeamItem::message(w, m) })
        .collect();
    items.extend(work.iter().filter(|x| x.asked.is_none()).map(TeamItem::work));
    items.extend(v.answers(channel, now).iter().map(TeamItem::answer));
    items.sort_by_key(|i| i.at);
    let mut replies: BTreeMap<String, (usize, At)> = BTreeMap::new();
    for i in &items {
        if let Some(t) = &i.thread {
            let e = replies.entry(t.clone()).or_insert((0, i.at));
            e.0 += 1;
            e.1 = i.at;
        }
    }
    for i in &mut items {
        if let Some((n, last)) = replies.get(&i.id) {
            (i.replies, i.last_reply) = (Some(*n), Some(*last));
        }
    }
    items.retain(|i| i.deleted != Some(true) || i.replies.is_some());
    items
}

/// What was said in a channel, for this person: its messages and the
/// agent's answers, in order, nothing deleted.
pub fn said<R: Runs>(v: &MemberView<R>, channel: &str, now: At) -> Vec<TeamItem> {
    let w = v.workspace();
    let mut items: Vec<TeamItem> = v.messages(channel).into_iter().map(|m| TeamItem::message(w, m)).collect();
    items.extend(v.answers(channel, now).iter().map(TeamItem::answer));
    items.sort_by_key(|i| i.at);
    items
}
