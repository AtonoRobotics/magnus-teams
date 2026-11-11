//! What makes a workspace a team chat: edits, reactions, pins, direct
//! messages, read markers, per-channel settings, the task board, project
//! files and invites. All of it travels as one team event shape, `chat`,
//! so it is content and leaves with the workspace.
use super::{Role, Workspace};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

type At = chrono::DateTime<chrono::Utc>;

/// How the team agent takes part in a channel. It always answers when
/// mentioned or named; `WhenHelpful` also lets it join on its own.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    #[default]
    Mentions,
    WhenHelpful,
}

/// What a member is told about a channel.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Notify {
    #[default]
    All,
    Mentions,
    Off,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Todo,
    Doing,
    Done,
}

/// Where a task came from: the message it was made from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Origin {
    pub channel: String,
    pub message: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Task {
    pub id: String,
    #[serde(default)]
    pub project: Option<String>,
    pub title: String,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub from: Option<Origin>,
    pub by: String,
    pub at: Option<At>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Invite {
    pub code: String,
    pub role: Option<Role>,
    /// Bound to one address, or open to whoever has the link.
    #[serde(default)]
    pub email: Option<String>,
    pub by: String,
    pub expires: Option<At>,
}

/// One change. A retired form keeps its reader here.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(tag = "what", rename_all = "snake_case")]
pub enum Chat {
    /// What an erased event reads as.
    #[default]
    Nothing,
    Edited {
        channel: String,
        id: String,
        text: String,
        at: At,
    },
    Reacted {
        channel: String,
        id: String,
        emoji: String,
        person: String,
        on: bool,
    },
    Pinned {
        channel: String,
        id: String,
        pinned: bool,
    },
    AgentMode {
        channel: String,
        mode: AgentMode,
    },
    Notify {
        channel: String,
        person: String,
        level: Notify,
    },
    Read {
        channel: String,
        person: String,
        at: At,
    },
    /// A channel only these people see. `agent` makes it a conversation
    /// with the team agent, where every message is addressed to it.
    Direct {
        channel: String,
        people: Vec<String>,
        agent: bool,
    },
    /// The member's message that the team agent took on as this task.
    Asked {
        channel: String,
        message: String,
        task: String,
    },
    /// The agent followed up on a thread that went quiet.
    FollowedUp {
        channel: String,
        thread: String,
    },
    Task {
        task: Task,
    },
    TaskRemoved {
        id: String,
    },
    /// A team file filed under a project, or none.
    FileIn {
        name: String,
        project: Option<String>,
    },
    Invite {
        invite: Invite,
    },
    InviteEnded {
        code: String,
    },
    /// An owner's limit on what the team agent spends in a calendar month,
    /// in dollars: the workspace's with no channel, else that channel's.
    /// No amount lifts it.
    SpendLimit {
        #[serde(default)]
        channel: Option<String>,
        #[serde(default)]
        usd: Option<f64>,
    },
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Direct {
    pub people: Vec<String>,
    pub agent: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct ChatState {
    pub modes: BTreeMap<String, AgentMode>,
    /// Person, then channel.
    pub notify: BTreeMap<String, BTreeMap<String, Notify>>,
    /// Person, then channel, then when they last read it.
    pub read: BTreeMap<String, BTreeMap<String, At>>,
    pub direct: BTreeMap<String, Direct>,
    pub edited: BTreeSet<String>,
    /// Message, then emoji, then who.
    pub reactions: BTreeMap<String, BTreeMap<String, BTreeSet<String>>>,
    pub pins: BTreeSet<String>,
    /// Task, then the message that asked for it.
    pub asked: BTreeMap<String, String>,
    pub followed_up: BTreeSet<String>,
    pub tasks: BTreeMap<String, Task>,
    pub file_projects: BTreeMap<String, String>,
    pub invites: BTreeMap<String, Invite>,
    /// The workspace's monthly spend limit in dollars, if an owner set one.
    pub limit_usd: Option<f64>,
    /// Each channel's monthly spend limit in dollars.
    pub channel_limits_usd: BTreeMap<String, f64>,
}

pub(super) fn apply(w: &mut Workspace, c: &Chat) {
    let s = &mut w.chat;
    match c {
        Chat::Nothing => {}
        Chat::Edited { channel, id, text, .. } => {
            if let Some(m) = w.channels.get_mut(channel).and_then(|ch| ch.messages.iter_mut().find(|m| &m.id == id)) {
                m.text = text.clone();
                s.edited.insert(id.clone());
            }
        }
        Chat::Reacted { id, emoji, person, on, .. } => {
            let by = s.reactions.entry(id.clone()).or_default();
            let who = by.entry(emoji.clone()).or_default();
            if *on {
                who.insert(person.clone());
            } else {
                who.remove(person);
            }
            by.retain(|_, w| !w.is_empty());
        }
        Chat::Pinned { id, pinned, .. } => {
            if *pinned {
                s.pins.insert(id.clone());
            } else {
                s.pins.remove(id);
            }
        }
        Chat::AgentMode { channel, mode } => {
            s.modes.insert(channel.clone(), *mode);
        }
        Chat::Notify { channel, person, level } => {
            s.notify.entry(person.clone()).or_default().insert(channel.clone(), *level);
        }
        Chat::Read { channel, person, at } => {
            let mark = s.read.entry(person.clone()).or_default().entry(channel.clone()).or_insert(*at);
            *mark = (*mark).max(*at);
        }
        Chat::Direct { channel, people, agent } => {
            s.direct.insert(channel.clone(), Direct { people: people.clone(), agent: *agent });
        }
        Chat::Asked { message, task, .. } => {
            s.asked.insert(task.clone(), message.clone());
        }
        Chat::FollowedUp { thread, .. } => {
            s.followed_up.insert(thread.clone());
        }
        Chat::Task { task } => {
            s.tasks.insert(task.id.clone(), task.clone());
        }
        Chat::TaskRemoved { id } => {
            s.tasks.remove(id);
        }
        Chat::FileIn { name, project } => match project {
            Some(p) => {
                s.file_projects.insert(name.clone(), p.clone());
            }
            None => {
                s.file_projects.remove(name);
            }
        },
        Chat::Invite { invite } => {
            s.invites.insert(invite.code.clone(), invite.clone());
        }
        Chat::InviteEnded { code } => {
            s.invites.remove(code);
        }
        Chat::SpendLimit { channel: None, usd } => s.limit_usd = *usd,
        Chat::SpendLimit { channel: Some(channel), usd: Some(usd) } => {
            s.channel_limits_usd.insert(channel.clone(), *usd);
        }
        Chat::SpendLimit { channel: Some(channel), usd: None } => {
            s.channel_limits_usd.remove(channel);
        }
    }
}

impl Workspace {
    /// Whether this person sees the channel: a direct conversation only its
    /// people do, a project's channel only the project's people, any other
    /// every member. Every route to a channel and its messages asks this.
    pub fn sees(&self, channel: &str, person: &str) -> bool {
        let Some(c) = self.channels.get(channel) else { return false };
        self.chat.direct.get(channel).is_none_or(|d| d.people.iter().any(|p| p == person)) && self.in_project(c.project.as_deref(), person)
    }

    /// Whether this person may read a team-volume file: it is there, and
    /// in no project or in one of theirs.
    pub fn sees_file(&self, name: &str, person: &str) -> bool {
        self.files.contains_key(name) && self.in_project(self.chat.file_projects.get(name).map(String::as_str), person)
    }

    pub fn agent_mode(&self, channel: &str) -> AgentMode {
        self.chat.modes.get(channel).copied().unwrap_or_default()
    }

    pub fn notify_level(&self, channel: &str, person: &str) -> Notify {
        self.chat.notify.get(person).and_then(|c| c.get(channel)).copied().unwrap_or_default()
    }

    /// Who is in a channel. The team agent is in every one, from the start.
    pub fn channel_members(&self, channel: &str) -> Vec<String> {
        let mut out: Vec<String> = match self.chat.direct.get(channel) {
            Some(d) => d.people.clone(),
            None => self.members.keys().filter(|m| self.sees(channel, m)).cloned().collect(),
        };
        out.push(super::conversation::AGENT_ID.to_string());
        out
    }

    /// Messages since this person last read the channel, others' only, and
    /// how many of those mention them.
    pub fn unread(&self, channel: &str, person: &str, name: &str) -> (usize, usize) {
        let Some(ch) = self.channels.get(channel) else { return (0, 0) };
        let since = self.chat.read.get(person).and_then(|c| c.get(channel)).copied();
        let fresh = ch.messages.iter().filter(|m| m.sender != person && since.is_none_or(|s| m.at > s));
        fresh.fold((0, 0), |(n, at), m| (n + 1, at + usize::from(mentions(&m.text, person, name))))
    }

    /// The project's tasks done, of all of them.
    pub fn progress(&self, project: &str) -> (usize, usize) {
        let tasks = self.chat.tasks.values().filter(|t| t.project.as_deref() == Some(project));
        tasks.fold((0, 0), |(done, all), t| (done + usize::from(t.status == Status::Done), all + 1))
    }

    /// Who may see a project's things: its people. Things in no project,
    /// or in one since removed, are the whole workspace's.
    pub fn in_project(&self, project: Option<&str>, person: &str) -> bool {
        project.is_none_or(|p| self.projects.get(p).is_none_or(|d| d.may_read(person)))
    }

    /// An invite that still works, and the workspace it is for.
    pub fn invite(&self, code: &str, now: At) -> Option<&Invite> {
        self.chat.invites.get(code).filter(|i| i.expires.is_none_or(|e| e > now))
    }
}

/// Whether a message mentions this person by id or name.
pub fn mentions(text: &str, person: &str, name: &str) -> bool {
    let lower = text.to_lowercase();
    [person, name].iter().filter(|n| !n.is_empty()).any(|n| {
        let tag = format!("@{}", n.to_lowercase());
        lower.match_indices(&tag).any(|(i, _)| lower[i + tag.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric()))
    })
}

#[cfg(test)]
mod tests {
    use super::super::{Change, Ledger, Message};
    use super::*;

    fn ws() -> Ledger {
        let mut l = Ledger::default();
        l.apply(&Change::Created { workspace: "w".into(), name: "Team".into(), owner: "alice".into() });
        l.apply(&Change::Member { workspace: "w".into(), person: "bob".into(), role: Role::Member });
        l.apply(&Change::Channel { workspace: "w".into(), channel: "c".into(), name: "general".into() });
        l
    }

    fn chat(l: &mut Ledger, c: Chat) {
        l.apply(&Change::Chat { workspace: "w".into(), chat: c });
    }

    fn say(l: &mut Ledger, id: &str, by: &str, text: &str, at: At) {
        let message = Message { id: id.into(), sender: by.into(), text: text.into(), thread: None, at };
        l.apply(&Change::Message { workspace: "w".into(), channel: "c".into(), message });
    }

    #[test]
    fn the_team_agent_is_in_every_channel_and_every_direct_message() {
        let mut l = ws();
        assert!(l.workspace("w").unwrap().channel_members("c").contains(&"agent".to_string()));
        l.apply(&Change::Channel { workspace: "w".into(), channel: "d".into(), name: String::new() });
        chat(&mut l, Chat::Direct { channel: "d".into(), people: vec!["alice".into(), "bob".into()], agent: false });
        let w = l.workspace("w").unwrap();
        assert_eq!(w.channel_members("d"), vec!["alice", "bob", "agent"]);
    }

    #[test]
    fn a_direct_message_is_seen_only_by_its_people() {
        let mut l = ws();
        l.apply(&Change::Channel { workspace: "w".into(), channel: "d".into(), name: String::new() });
        chat(&mut l, Chat::Direct { channel: "d".into(), people: vec!["alice".into()], agent: true });
        let w = l.workspace("w").unwrap();
        assert!(w.sees("d", "alice") && !w.sees("d", "bob") && w.sees("c", "bob"));
    }

    /// A project's channel and files are its people's, not the workspace's
    /// (spec rev 142: each project has its channels, files and members).
    #[test]
    fn a_projects_channel_and_files_are_seen_only_by_its_people() {
        let mut l = ws();
        l.apply(&Change::Member { workspace: "w".into(), person: "gus".into(), role: Role::Guest });
        let def = super::super::knowledge::Project { id: "p".into(), name: "P".into(), owner: "alice".into(), ..Default::default() };
        l.apply(&Change::Project { workspace: "w".into(), id: "p".into(), def });
        l.apply(&Change::ChannelProject { workspace: "w".into(), channel: "c".into(), project: Some("p".into()) });
        l.apply(&Change::File { workspace: "w".into(), file: super::super::File { name: "plan.pdf".into(), size: 1, by: "alice".into() } });
        chat(&mut l, Chat::FileIn { name: "plan.pdf".into(), project: Some("p".into()) });
        let w = l.workspace("w").unwrap();
        assert!(w.sees("c", "alice") && w.sees_file("plan.pdf", "alice"));
        for outsider in ["bob", "gus"] {
            assert!(!w.sees("c", outsider), "{outsider} sees a project channel they are not in");
            assert!(!w.sees_file("plan.pdf", outsider), "{outsider} reads a project file they are not in");
        }
        assert_eq!(w.channel_members("c"), vec!["alice", "agent"]);
    }

    #[test]
    fn an_edit_changes_the_text_and_marks_it() {
        let mut l = ws();
        let now = chrono::Utc::now();
        say(&mut l, "m", "bob", "helo", now);
        chat(&mut l, Chat::Edited { channel: "c".into(), id: "m".into(), text: "hello".into(), at: now });
        let w = l.workspace("w").unwrap();
        assert_eq!(w.channels["c"].messages[0].text, "hello");
        assert!(w.chat.edited.contains("m"));
    }

    #[test]
    fn reactions_toggle_and_empty_ones_go() {
        let mut l = ws();
        chat(&mut l, Chat::Reacted { channel: "c".into(), id: "m".into(), emoji: "👍".into(), person: "bob".into(), on: true });
        assert_eq!(l.workspace("w").unwrap().chat.reactions["m"]["👍"].len(), 1);
        chat(&mut l, Chat::Reacted { channel: "c".into(), id: "m".into(), emoji: "👍".into(), person: "bob".into(), on: false });
        assert!(l.workspace("w").unwrap().chat.reactions["m"].is_empty());
    }

    #[test]
    fn unread_counts_others_messages_since_the_last_read_and_mentions() {
        let mut l = ws();
        let t0 = chrono::Utc::now();
        say(&mut l, "1", "alice", "hi all", t0);
        say(&mut l, "2", "alice", "@bob can you look", t0 + chrono::Duration::seconds(1));
        say(&mut l, "3", "bob", "mine", t0 + chrono::Duration::seconds(2));
        assert_eq!(l.workspace("w").unwrap().unread("c", "bob", "Bob"), (2, 1));
        chat(&mut l, Chat::Read { channel: "c".into(), person: "bob".into(), at: t0 });
        assert_eq!(l.workspace("w").unwrap().unread("c", "bob", "Bob"), (1, 1));
        chat(&mut l, Chat::Read { channel: "c".into(), person: "bob".into(), at: t0 - chrono::Duration::seconds(5) });
        assert_eq!(l.workspace("w").unwrap().unread("c", "bob", "Bob"), (1, 1), "a read marker never moves back");
    }

    #[test]
    fn a_mention_is_a_whole_name() {
        assert!(mentions("ping @bob.", "bob", "Bob"));
        assert!(mentions("@Bob Smith?", "b1", "Bob Smith"));
        assert!(!mentions("ping @bobby", "bob", "Bob"));
        assert!(!mentions("bob, look", "bob", "Bob"));
    }

    #[test]
    fn progress_counts_a_projects_done_tasks() {
        let mut l = ws();
        for (id, status) in [("a", Status::Done), ("b", Status::Doing), ("c", Status::Todo)] {
            let task = Task { id: id.into(), project: Some("p".into()), title: id.into(), status, by: "bob".into(), ..Default::default() };
            chat(&mut l, Chat::Task { task });
        }
        chat(&mut l, Chat::TaskRemoved { id: "c".into() });
        assert_eq!(l.workspace("w").unwrap().progress("p"), (1, 2));
    }

    #[test]
    fn an_erased_chat_event_reads_as_nothing() {
        let c: Change = serde_json::from_value(serde_json::json!({ "change": "chat", "workspace": "w" })).unwrap();
        assert_eq!(c, Change::Chat { workspace: "w".into(), chat: Chat::Nothing });
    }

    #[test]
    fn an_expired_invite_no_longer_works() {
        let mut l = ws();
        let now = chrono::Utc::now();
        let invite = Invite { code: "k".into(), role: Some(Role::Member), email: None, by: "alice".into(), expires: Some(now) };
        chat(&mut l, Chat::Invite { invite });
        let w = l.workspace("w").unwrap();
        assert!(w.invite("k", now - chrono::Duration::seconds(1)).is_some());
        assert!(w.invite("k", now).is_none());
    }
}
