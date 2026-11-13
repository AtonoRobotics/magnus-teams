//! What one member sees of a workspace, in one place. Every route that shows
//! a channel, a message, an answer, a file or a search result asks this,
//! so the rules hold the same everywhere:
//! - a channel is seen by its people: a direct conversation's, a project's
//!   members', else every member (`Workspace::sees`);
//! - what a person deleted a conversation for themselves up to is gone for
//!   them (`kept_for`);
//! - a message or answer deleted for everyone is gone for everyone (`unsent`);
//! - an answer built from data only its requester may see is theirs: everyone
//!   else is told only that it answered them (`answer_as`).
//!
//! It reads the ledger and each request's outcome, and nothing else, so it is
//! tested with no server and no database.
use super::conversation::{is_long, placement, AGENT_ID};
use super::{Channel, File, Ledger, Message, Workspace};
use crate::state::Status;
use std::collections::BTreeMap;

type At = chrono::DateTime<chrono::Utc>;

/// How an agent's answer is named among a channel's messages: by its request.
pub(crate) const ANSWER_PREFIX: &str = "agent:";
/// How much of the asking message names its work card.
const MAX_TITLE_CHARS: usize = 80;

/// What a request came to, as far as the channel shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub status: Status,
    pub summary: Option<String>,
    pub reason: Option<String>,
    /// It read data only its requester may see.
    pub private: bool,
    pub started: At,
    pub ended: Option<At>,
    pub steps: usize,
    /// The last tool it called: what it is doing now.
    pub doing: Option<String>,
}

/// Where each request's outcome is read from: the kernel's state, or a
/// table in a test.
pub trait Runs {
    fn outcome(&self, task: &str) -> Option<Outcome>;
}

impl Runs for crate::state::State {
    fn outcome(&self, task: &str) -> Option<Outcome> {
        let t = self.tasks.get(task)?;
        let run = self.runs.get(&t.run_id)?;
        let doing = run.replies.iter().rev().flat_map(|r| r.blocks.iter().rev()).find_map(|b| match b {
            magnus_adapter_proto::Block::ToolCall { name, .. } => Some(name.clone()),
            _ => None,
        });
        Some(Outcome {
            status: run.status,
            summary: run.summary.clone(),
            reason: run.reason.clone(),
            private: run.private,
            started: t.created_at,
            ended: run.ended_at,
            steps: run.replies.len(),
            doing,
        })
    }
}

impl Runs for BTreeMap<String, Outcome> {
    fn outcome(&self, task: &str) -> Option<Outcome> {
        self.get(task).cloned()
    }
}

/// What the agent answers in the thread once a request ends: its result,
/// or why it could not finish.
pub(crate) fn answer_text(o: &Outcome) -> Option<String> {
    match o.status {
        Status::Completed => o.summary.clone(),
        Status::Failed => Some(format!("I could not finish this: {}", o.reason.as_deref().unwrap_or("it failed"))),
        _ => None,
    }
}

/// A finished request's answer as one viewer may read it, and whether it is
/// the requester's alone. A run that read data only its requester may see
/// answers them alone (spec rev 142); the thread sees that it answered.
pub(crate) fn answer_as(private: bool, text: String, requester: &str, viewer: &str) -> (String, bool) {
    match (private, viewer == requester) {
        (false, _) => (text, false),
        (true, true) => (text, true),
        (true, false) => (format!("I answered {requester} directly: the answer holds data only they may see."), true),
    }
}

/// Who wrote a message, and for whom when it was the agent: its posts are
/// sent as `agent:<requester>`.
pub(crate) fn author(sender: &str) -> (&str, Option<&str>) {
    match sender.strip_prefix(ANSWER_PREFIX) {
        Some(r) => (AGENT_ID, Some(r)),
        None => (sender, None),
    }
}

/// The agent's answer to one request, as this viewer reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    /// `agent:<task>`.
    pub id: String,
    pub task: String,
    pub requester: String,
    pub text: String,
    /// Where it shows: the thread it was asked in, or the asking message's
    /// own thread when the work was long.
    pub thread: Option<String>,
    pub at: At,
    /// The requester's alone: they see it, others see that it answered.
    pub private: bool,
}

/// A request's live card.
#[derive(Clone, Debug, PartialEq)]
pub struct Work {
    pub task: String,
    pub requester: String,
    pub status: Status,
    pub steps: usize,
    pub doing: Option<String>,
    /// The asking message's words, cut short.
    pub title: String,
    pub long: bool,
    /// Where its work and answer show.
    pub thread: Option<String>,
    /// The thread it was asked in, if any.
    pub asked_in: Option<String>,
    pub started: At,
    pub reason: Option<String>,
    /// The message that asked for it, which carries the card.
    pub asked: Option<String>,
}

/// One message or answer a person may act on.
#[derive(Clone, Debug, PartialEq)]
pub enum Said<'a> {
    Message(&'a Message),
    Answer(Answer),
}

impl Said<'_> {
    pub fn id(&self) -> &str {
        match self {
            Said::Message(m) => &m.id,
            Said::Answer(a) => &a.id,
        }
    }

    /// Who wrote it: a person, or the agent.
    pub fn author(&self) -> &str {
        match self {
            Said::Message(m) => author(&m.sender).0,
            Said::Answer(_) => AGENT_ID,
        }
    }

    /// Whom the agent wrote it for, when it did.
    pub fn requester(&self) -> Option<&str> {
        match self {
            Said::Message(m) => author(&m.sender).1,
            Said::Answer(a) => Some(&a.requester),
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Said::Message(m) => &m.text,
            Said::Answer(a) => &a.text,
        }
    }

    pub fn at(&self) -> At {
        match self {
            Said::Message(m) => m.at,
            Said::Answer(a) => a.at,
        }
    }

    /// An answer holding data only its requester may see.
    pub fn private(&self) -> bool {
        matches!(self, Said::Answer(a) if a.private)
    }

    /// The request it answers, if it is an answer.
    pub fn task(&self) -> Option<&str> {
        match self {
            Said::Message(_) => None,
            Said::Answer(a) => Some(&a.task),
        }
    }

    /// As a channel message: an answer is its requester's, sent as the agent.
    pub fn as_message(&self) -> Message {
        match self {
            Said::Message(m) => (*m).clone(),
            Said::Answer(a) => Message { id: a.id.clone(), sender: format!("{ANSWER_PREFIX}{}", a.requester), text: a.text.clone(), thread: a.thread.clone(), at: a.at },
        }
    }
}

/// A workspace as one member sees it.
pub struct MemberView<'a, R: Runs> {
    ledger: &'a Ledger,
    w: &'a Workspace,
    person: &'a str,
    runs: &'a R,
}

impl<'a> MemberView<'a, crate::state::State> {
    /// The workspace as this person sees it, if they are in it.
    pub fn of(s: &'a crate::state::State, ws: &str, person: &'a str) -> Option<Self> {
        let w = s.team.workspace(ws).filter(|w| w.members.contains_key(person))?;
        Some(MemberView::new(&s.team, w, person, s))
    }
}

impl<'a, R: Runs> MemberView<'a, R> {
    pub fn new(ledger: &'a Ledger, w: &'a Workspace, person: &'a str, runs: &'a R) -> Self {
        MemberView { ledger, w, person, runs }
    }

    pub fn workspace(&self) -> &'a Workspace {
        self.w
    }

    pub fn person(&self) -> &'a str {
        self.person
    }

    /// Whether the channel is there and this person sees it.
    pub fn sees(&self, channel: &str) -> bool {
        self.w.sees(channel, self.person)
    }

    /// The channels this person sees.
    pub fn channels(&self) -> impl Iterator<Item = (&'a String, &'a Channel)> + '_ {
        self.w.channels.iter().filter(|(id, _)| self.sees(id))
    }

    /// The team-volume files this person may read.
    pub fn files(&self) -> impl Iterator<Item = &'a File> + '_ {
        self.w.files.values().filter(|f| self.w.sees_file(&f.name, self.person))
    }

    fn kept(&self, channel: &str, at: At) -> bool {
        self.w.kept_for(channel, self.person, at)
    }

    fn unsent(&self, id: &str) -> bool {
        self.w.chat.unsent.contains(id)
    }

    /// Every message still in the channel for this person, those deleted for
    /// everyone among them: where one stood, its replies still hang from it.
    pub fn places(&self, channel: &str) -> Vec<&'a Message> {
        if !self.sees(channel) {
            return vec![];
        }
        self.w.channels.get(channel).map(|c| c.messages.iter().filter(|m| self.kept(channel, m.at)).collect()).unwrap_or_default()
    }

    /// The channel's messages this person has, none deleted.
    pub fn messages(&self, channel: &str) -> Vec<&'a Message> {
        self.places(channel).into_iter().filter(|m| !self.unsent(&m.id)).collect()
    }

    /// Whether a message was deleted for everyone.
    pub fn deleted(&self, id: &str) -> bool {
        self.unsent(id)
    }

    /// Each request's live card in the channel. One on an asking message goes
    /// with that message; one on none, from when it began.
    pub fn work(&self, channel: &str, now: At) -> Vec<Work> {
        if !self.sees(channel) {
            return vec![];
        }
        let texts: BTreeMap<&str, &str> = self.w.channels.get(channel).map(|c| c.messages.iter().map(|m| (m.id.as_str(), m.text.as_str())).collect()).unwrap_or_default();
        let mut out = vec![];
        for r in self.ledger.runs_in(&self.w.id, channel) {
            let Some(o) = self.runs.outcome(&r.task) else { continue };
            let asked = self.w.chat.asked.get(&r.task).cloned();
            if asked.is_none() && !self.kept(channel, o.started) {
                continue;
            }
            let long = is_long(o.started, o.ended.unwrap_or(now), o.steps);
            let title = asked.as_deref().and_then(|m| texts.get(m)).map_or("Following up", |t| t).chars().take(MAX_TITLE_CHARS).collect();
            out.push(Work {
                task: r.task.clone(),
                requester: r.requester.clone(),
                status: o.status,
                steps: o.steps,
                doing: o.doing.clone(),
                title,
                long,
                thread: placement(r.thread.as_deref(), asked.as_deref(), long),
                asked_in: r.thread.clone(),
                started: o.started,
                reason: o.reason.clone(),
                asked,
            });
        }
        out
    }

    /// The agent's answers in the channel, as this person may read them.
    pub fn answers(&self, channel: &str, now: At) -> Vec<Answer> {
        if !self.sees(channel) {
            return vec![];
        }
        let mut out = vec![];
        for r in self.ledger.runs_in(&self.w.id, channel) {
            let Some(o) = self.runs.outcome(&r.task) else { continue };
            let id = format!("{ANSWER_PREFIX}{}", r.task);
            let Some(text) = answer_text(&o).filter(|_| !self.unsent(&id)) else { continue };
            let at = o.ended.unwrap_or(now);
            if !self.kept(channel, at) {
                continue;
            }
            let asked = self.w.chat.asked.get(&r.task).map(String::as_str);
            let thread = placement(r.thread.as_deref(), asked, is_long(o.started, at, o.steps));
            let (text, private) = answer_as(o.private, text, &r.requester, self.person);
            out.push(Answer { id, task: r.task.clone(), requester: r.requester.clone(), text, thread, at, private });
        }
        out
    }

    /// One message or answer, if this person has it and it was not deleted.
    pub fn message(&self, channel: &str, id: &str, now: At) -> Option<Said<'a>> {
        if let Some(m) = self.messages(channel).into_iter().find(|m| m.id == id) {
            return Some(Said::Message(m));
        }
        id.strip_prefix(ANSWER_PREFIX)?;
        self.answers(channel, now).into_iter().find(|a| a.id == id).map(Said::Answer)
    }
}
