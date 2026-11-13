//! The team chat's items as they travel to the app: a person's message, the
//! agent's live work, and its answer. Each says which kind of the chat
//! view's items it is (`kind`: the personal chat's `message` and `answer`,
//! with `work` and `deleted`) and the files it carries, so the app's one chat
//! view (desktop/ui/chat-view.js) reads it as it reads a personal chat entry
//! (decisions/2026-11-13-one-chat-view.md). Defined once here; the app's
//! JSDoc (`TeamMessage`, `TeamWork` in desktop/ui/team-chat.js) follows them.
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
    /// The task's status as the personal chat sends a task's: running,
    /// suspended, completed, failed or cancelled.
    pub status: crate::state::Status,
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
    /// The chat view's kind: `message` (a person's), `answer` (the agent's
    /// words), `work` (the agent's work with no words yet) or `deleted`.
    pub kind: &'static str,
    /// The files it carries, by name: its `📎` lines.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
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

impl TeamWork {
    pub fn of(w: &Work) -> TeamWork {
        TeamWork {
            task: w.task.clone(),
            requester: w.requester.clone(),
            status: w.status,
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

/// The files a message carries: its lines that begin with a paperclip.
pub fn attached(text: &str) -> Vec<String> {
    text.lines().filter_map(|l| l.strip_prefix("📎 ")).map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect()
}

impl TeamItem {
    fn bare(kind: &'static str, id: String, sender: String, requester: Option<String>, text: String, thread: Option<String>, at: At) -> TeamItem {
        TeamItem {
            id,
            kind,
            files: attached(&text),
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
        let deleted = w.chat.unsent.contains(&m.id);
        let kind = if deleted {
            "deleted"
        } else if sender == AGENT_ID {
            "answer"
        } else {
            "message"
        };
        TeamItem {
            edited: Some(w.chat.edited.contains(&m.id)),
            pinned: Some(w.chat.pins.contains(&m.id)),
            reactions: Some(w.chat.reactions.get(&m.id).cloned().unwrap_or_default()),
            deleted: Some(deleted),
            ..TeamItem::bare(kind, m.id.clone(), sender.to_string(), requester.map(String::from), m.text.clone(), m.thread.clone(), m.at)
        }
    }

    /// A request's work that no message carries: one the agent began itself.
    pub fn work(w: &Work) -> TeamItem {
        TeamItem {
            work: Some(TeamWork::of(w)),
            ..TeamItem::bare("work", format!("work:{}", w.task), AGENT_ID.into(), Some(w.requester.clone()), String::new(), w.asked_in.clone(), w.started)
        }
    }

    /// The agent's answer, as the viewer may read it.
    pub fn answer(a: &Answer) -> TeamItem {
        TeamItem {
            task: Some(a.task.clone()),
            private: Some(a.private),
            ..TeamItem::bare("answer", a.id.clone(), AGENT_ID.into(), Some(a.requester.clone()), a.text.clone(), a.thread.clone(), a.at)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> At {
        chrono::DateTime::from_timestamp(0, 0).unwrap_or_default()
    }

    /// Each item says which of the chat view's kinds it is, and its files,
    /// so the app reads it as a personal chat entry.
    #[test]
    fn an_item_is_one_of_the_chat_views_kinds_with_its_files() {
        let said = TeamItem::bare("message", "m1".into(), "sam".into(), None, "see this\n📎 plan.pdf\n📎  \n📎 shot.png".into(), None, at());
        assert_eq!(said.files, ["plan.pdf", "shot.png"]);
        let v = serde_json::to_value(&said).unwrap_or_default();
        assert_eq!(v["kind"], "message");
        assert_eq!(v["files"], serde_json::json!(["plan.pdf", "shot.png"]));
        let quiet = serde_json::to_value(TeamItem::bare("answer", "a1".into(), AGENT_ID.into(), None, "done".into(), None, at())).unwrap_or_default();
        assert_eq!(quiet["kind"], "answer");
        assert!(quiet.get("files").is_none(), "an item with no files says it has some");
    }
}
