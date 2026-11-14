//! Team workspaces: channels, roles, a team volume and one team agent. The
//! agent acts for whoever asked and never for the channel, so its authority
//! is the workspace's grants cut down to what that member's role allows.
macro_rules! ok_or_return {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(r) => return *r,
        }
    };
}

mod agent;
pub mod chat;
mod chat_routes;
pub mod conversation;
mod forget;
#[cfg(test)]
mod forget_tests;
#[cfg(test)]
mod invite_tests;
pub mod invites;
pub mod items;
mod join_page;
mod kb_routes;
mod keep;
#[cfg(test)]
mod keep_tests;
pub(crate) mod knowledge;
pub mod may;
pub mod member_view;
#[cfg(test)]
mod member_view_tests;
pub(crate) mod membership;
#[cfg(test)]
mod membership_tests;
#[cfg(test)]
mod message_action_tests;
mod message_actions;
mod notices;
mod own_view;
#[cfg(test)]
mod own_view_tests;
pub mod owner;
#[cfg(test)]
mod owner_tests;
mod presence;
#[cfg(test)]
mod privacy_tests;
mod project_routes;
mod routes;
pub(crate) mod routines;
mod search;
pub mod spend;
#[cfg(test)]
mod talk_tests;
pub mod team_admin;
pub(crate) mod upkeep;
mod whisper;

pub(crate) use agent::signs_in_per_person;
pub(crate) use agent::team_specs as specs;
pub use agent::BUILTINS;
pub use routes::router;
pub use routines::schedule;
pub use upkeep::{pack_files, unpack_files};

use knowledge::{Entry, Project, Weight};
use routines::Routine;
use whisper::Whisper;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Lower roles come first. A ceiling is the lower of two.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Guest,
    Member,
    Admin,
    Owner,
}

/// One change to a workspace. A retired form keeps its reader here.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    Created {
        workspace: String,
        #[serde(default)]
        name: String,
        owner: String,
    },
    Deleted {
        workspace: String,
    },
    Member {
        workspace: String,
        person: String,
        role: Role,
    },
    Removed {
        workspace: String,
        person: String,
    },
    Channel {
        workspace: String,
        channel: String,
        #[serde(default)]
        name: String,
    },
    Message {
        workspace: String,
        channel: String,
        #[serde(default)]
        message: Message,
    },
    /// An app granted to the workspace, for these roles. Empty takes it away.
    Connector {
        workspace: String,
        app: String,
        roles: Vec<Role>,
    },
    /// A request the team agent took on, and the channel as it then stood.
    Run {
        workspace: String,
        channel: String,
        thread: Option<String>,
        task: String,
        requester: String,
        #[serde(default)]
        history: String,
        /// A sub-agent's: it works in the thread and does not answer there.
        #[serde(default)]
        sub: bool,
        /// A guest's words are among what it was given.
        #[serde(default)]
        outside: bool,
    },
    File {
        workspace: String,
        #[serde(default)]
        file: File,
    },
    FileRemoved {
        workspace: String,
        #[serde(default)]
        name: String,
    },
    /// A file built from the requester's own data, kept back from the volume.
    Held {
        workspace: String,
        person: String,
        #[serde(default)]
        file: File,
    },
    /// The person shared it, or threw it away.
    Released {
        workspace: String,
        person: String,
        #[serde(default)]
        name: String,
    },
    /// Retired for `Entry`; old ones read as workspace-level entries.
    Fact {
        workspace: String,
        id: String,
        #[serde(default)]
        text: String,
        by: String,
    },
    FactRemoved {
        workspace: String,
        id: String,
    },
    /// The most any member's own computer may be set to while in this workspace.
    LocalCeiling {
        workspace: String,
        mode: crate::local::Mode,
    },
    /// Retired by spec rev 140, where only the person who asked answers an
    /// ask: it named who else answered asks that used a workspace resource.
    /// An old log still holds it, and it is read and changes nothing.
    Approvers {
        workspace: String,
        resource: String,
        people: Vec<String>,
    },
    Note {
        workspace: String,
        task: String,
        #[serde(default)]
        note: Note,
    },
    Project {
        workspace: String,
        id: String,
        #[serde(default)]
        def: Project,
    },
    /// An admin deleted it with the person who owned it (Admin, People).
    ProjectRemoved {
        workspace: String,
        id: String,
    },
    ChannelProject {
        workspace: String,
        channel: String,
        project: Option<String>,
    },
    /// An entry as it now stands: written, corrected, flagged or retired.
    Entry {
        workspace: String,
        id: String,
        #[serde(default)]
        entry: Entry,
    },
    EntryRemoved {
        workspace: String,
        id: String,
    },
    Whisper {
        workspace: String,
        channel: String,
        thread: Option<String>,
        task: String,
        owner: String,
        #[serde(default)]
        history: String,
    },
    WhisperShared {
        workspace: String,
        task: String,
    },
    Routine {
        workspace: String,
        id: String,
        #[serde(default)]
        routine: Routine,
    },
    RoutineRemoved {
        workspace: String,
        id: String,
    },
    RoutineRan {
        workspace: String,
        id: String,
        task: String,
        at: chrono::DateTime<chrono::Utc>,
    },
    /// Messaging, the board, invites and per-channel settings: see `chat`.
    Chat {
        workspace: String,
        #[serde(default)]
        chat: chat::Chat,
    },
    /// A Magnus admin made someone a Team Admin, who starts workspaces, or
    /// took it back (`team_admin`).
    TeamAdmin {
        person: String,
        on: bool,
    },
    /// Once, when Team Admin came in: everyone who owned a workspace then
    /// became a Team Admin.
    TeamAdminsSeeded {
        people: Vec<String>,
    },
}

/// Scratch for one request: never a fact, and gone when the task is over.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Note {
    pub id: String,
    pub text: String,
    pub by: String,
    pub at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Message {
    pub id: String,
    pub sender: String,
    pub text: String,
    #[serde(default)]
    pub thread: Option<String>,
    pub at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct File {
    pub name: String,
    pub size: u64,
    /// The member it was made for or by.
    pub by: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TeamRun {
    pub workspace: String,
    pub channel: String,
    pub thread: Option<String>,
    pub task: String,
    pub requester: String,
    pub history: String,
    pub sub: bool,
    pub outside: bool,
}

impl From<TeamRun> for Change {
    fn from(t: TeamRun) -> Change {
        Change::Run { workspace: t.workspace, channel: t.channel, thread: t.thread, task: t.task, requester: t.requester, history: t.history, sub: t.sub, outside: t.outside }
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub members: BTreeMap<String, Role>,
    pub channels: BTreeMap<String, Channel>,
    pub connectors: BTreeMap<String, Vec<Role>>,
    pub files: BTreeMap<String, File>,
    /// Person, then file name.
    pub held: BTreeMap<String, BTreeMap<String, File>>,
    /// Every entry, of the workspace's own base and of each project's.
    pub knowledge: BTreeMap<String, Entry>,
    pub projects: BTreeMap<String, Project>,
    pub local_ceiling: Option<crate::local::Mode>,
    pub routines: BTreeMap<String, Routine>,
    pub chat: chat::ChatState,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Channel {
    pub name: String,
    pub messages: Vec<Message>,
    /// The project this channel's work belongs to.
    pub project: Option<String>,
}

#[derive(Default, Clone, Debug)]
pub struct Ledger {
    workspaces: BTreeMap<String, Workspace>,
    runs: BTreeMap<String, TeamRun>,
    /// A task, then its working notes.
    notes: BTreeMap<String, Vec<Note>>,
    whispers: BTreeMap<String, Whisper>,
    /// A message, then the events that hold its text: its own and its edits.
    seqs: BTreeMap<String, Vec<i64>>,
    pub(crate) admins: team_admin::Admins,
}

impl Ledger {
    /// `apply`, noting which events hold each message's text, so deleting a
    /// message can take that text out of the log.
    pub fn apply_at(&mut self, c: &Change, seq: i64) {
        let held = match c {
            Change::Message { message, .. } => Some(&message.id),
            Change::Chat { chat: chat::Chat::Edited { id, .. }, .. } => Some(id),
            _ => None,
        };
        if let Some(id) = held.filter(|id| !id.is_empty()) {
            self.seqs.entry(id.clone()).or_default().push(seq);
        }
        self.apply(c);
    }

    /// The events that hold a message's text.
    pub fn message_seqs(&self, id: &str) -> &[i64] {
        self.seqs.get(id).map_or(&[], Vec::as_slice)
    }

    pub fn apply(&mut self, c: &Change) {
        if let Change::Created { workspace, name, owner } = c {
            let members = BTreeMap::from([(owner.clone(), Role::Owner)]);
            self.workspaces.insert(workspace.clone(), Workspace { id: workspace.clone(), name: name.clone(), members, ..Default::default() });
            return;
        }
        if let Change::Run { workspace, channel, thread, task, requester, history, sub, outside } = c {
            let run = TeamRun {
                workspace: workspace.clone(),
                channel: channel.clone(),
                thread: thread.clone(),
                task: task.clone(),
                requester: requester.clone(),
                history: history.clone(),
                sub: *sub,
                outside: *outside,
            };
            self.runs.insert(task.clone(), run);
            return;
        }
        if let Change::Note { task, note, .. } = c {
            // One whose content expired with its task reads as nothing.
            if !note.id.is_empty() {
                self.notes.entry(task.clone()).or_default().push(note.clone());
            }
            return;
        }
        if let Change::Whisper { workspace, channel, thread, task, owner, history } = c {
            let w = Whisper {
                workspace: workspace.clone(),
                channel: channel.clone(),
                thread: thread.clone(),
                task: task.clone(),
                owner: owner.clone(),
                history: history.clone(),
                shared: false,
            };
            self.whispers.insert(task.clone(), w);
            return;
        }
        if let Change::WhisperShared { task, .. } = c {
            if let Some(w) = self.whispers.get_mut(task) {
                w.shared = true;
            }
            return;
        }
        if let Change::Deleted { workspace } = c {
            self.workspaces.remove(workspace);
            return;
        }
        if self.admins.apply(c) {
            return;
        }
        match c {
            Change::Member { workspace, person, role } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.members.insert(person.clone(), *role);
                }
            }
            Change::Removed { workspace, person } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.members.remove(person);
                    w.held.remove(person);
                    // Out of every project too, so no project's list lets
                    // them back in (`Workspace::may_read`).
                    for d in w.projects.values_mut() {
                        d.members.retain(|m| m != person);
                        d.curators.retain(|c| c != person);
                    }
                }
            }
            Change::Channel { workspace, channel, name } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.channels.insert(channel.clone(), Channel { name: name.clone(), ..Default::default() });
                }
            }
            // One whose content was erased (its sender deleted it) reads as nothing.
            Change::Message { message, .. } if message.id.is_empty() => {}
            Change::Message { workspace, channel, message } => {
                if let Some(ch) = self.workspaces.get_mut(workspace).and_then(|w| w.channels.get_mut(channel)) {
                    ch.messages.push(message.clone());
                }
            }
            Change::Connector { workspace, app, roles } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    if roles.is_empty() {
                        w.connectors.remove(app);
                    } else {
                        w.connectors.insert(app.clone(), roles.clone());
                    }
                }
            }
            Change::File { workspace, file } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.files.insert(file.name.clone(), file.clone());
                }
            }
            Change::FileRemoved { workspace, name } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.files.remove(name);
                }
            }
            Change::Held { workspace, person, file } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.held.entry(person.clone()).or_default().insert(file.name.clone(), file.clone());
                }
            }
            Change::Released { workspace, person, name } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    if let Some(files) = w.held.get_mut(person) {
                        files.remove(name);
                    }
                }
            }
            Change::Fact { workspace, id, text, by } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    let entry = Entry {
                        id: id.clone(),
                        kind: "fact".into(),
                        statement: text.clone(),
                        source: "a member".into(),
                        weight: Weight::Member,
                        author: by.clone(),
                        owner: by.clone(),
                        ..Default::default()
                    };
                    w.knowledge.insert(id.clone(), entry);
                }
            }
            Change::FactRemoved { workspace, id } | Change::EntryRemoved { workspace, id } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.knowledge.remove(id);
                }
            }
            Change::Entry { workspace, id, entry } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.knowledge.insert(id.clone(), entry.clone());
                }
            }
            Change::LocalCeiling { workspace, mode } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.local_ceiling = Some(*mode);
                }
            }
            Change::Approvers { .. } => {}
            Change::Project { workspace, id, def } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.projects.insert(id.clone(), def.clone());
                }
            }
            Change::ProjectRemoved { workspace, id } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.projects.remove(id);
                    w.channels.values_mut().filter(|c| c.project.as_deref() == Some(id.as_str())).for_each(|c| c.project = None);
                }
            }
            Change::ChannelProject { workspace, channel, project } => {
                if let Some(ch) = self.workspaces.get_mut(workspace).and_then(|w| w.channels.get_mut(channel)) {
                    ch.project = project.clone();
                }
            }
            Change::Routine { workspace, id, routine } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.routines.insert(id.clone(), routine.clone());
                }
            }
            Change::RoutineRemoved { workspace, id } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.routines.remove(id);
                }
            }
            Change::RoutineRan { workspace, id, at, .. } => {
                if let Some(r) = self.workspaces.get_mut(workspace).and_then(|w| w.routines.get_mut(id)) {
                    r.last_at = Some(*at);
                }
            }
            Change::Chat { workspace, chat } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    chat::apply(w, chat);
                }
            }
            Change::Created { .. }
            | Change::Deleted { .. }
            | Change::Run { .. }
            | Change::Note { .. }
            | Change::Whisper { .. }
            | Change::WhisperShared { .. }
            | Change::TeamAdmin { .. }
            | Change::TeamAdminsSeeded { .. } => {}
        }
    }

    pub fn workspace(&self, id: &str) -> Option<&Workspace> {
        self.workspaces.get(id)
    }

    pub fn of(&self, person: &str) -> Vec<&Workspace> {
        self.workspaces.values().filter(|w| w.members.contains_key(person)).collect()
    }

    pub fn role(&self, workspace: &str, person: &str) -> Option<Role> {
        self.workspaces.get(workspace).and_then(|w| w.members.get(person)).copied()
    }

    /// The team request a task carries out, if it is one.
    pub fn run_of(&self, task: &str) -> Option<&TeamRun> {
        self.runs.get(task)
    }

    /// The requests that answer in a channel. A sub-agent answers its parent.
    pub fn runs_in(&self, workspace: &str, channel: &str) -> Vec<&TeamRun> {
        self.runs.values().filter(|r| r.workspace == workspace && r.channel == channel && !r.sub).collect()
    }

    pub fn all(&self) -> impl Iterator<Item = &Workspace> {
        self.workspaces.values()
    }

    /// Every request made in a workspace, sub-agents' too.
    pub fn runs_of(&self, workspace: &str) -> Vec<&TeamRun> {
        self.runs.values().filter(|r| r.workspace == workspace).collect()
    }

    /// The strictest ceiling any of this person's workspaces sets.
    pub fn local_ceiling(&self, person: &str) -> Option<crate::local::Mode> {
        self.of(person).into_iter().filter_map(|w| w.local_ceiling).min()
    }

    pub fn notes(&self, task: &str) -> &[Note] {
        self.notes.get(task).map_or(&[], Vec::as_slice)
    }

    /// The tasks that have working notes.
    pub fn noted_tasks(&self) -> impl Iterator<Item = &String> {
        self.notes.keys()
    }

    /// A task's notes expired with it.
    pub fn drop_notes(&mut self, task: &str) {
        self.notes.remove(task);
    }

    pub fn whisper_of(&self, task: &str) -> Option<&Whisper> {
        self.whispers.get(task)
    }

    pub fn whispers_by(&self, workspace: &str, channel: &str, person: &str) -> Vec<&Whisper> {
        self.whispers.values().filter(|w| w.workspace == workspace && w.channel == channel && w.owner == person).collect()
    }

    pub fn runs_by(&self, workspace: &str, person: &str) -> Vec<&TeamRun> {
        self.runs.values().filter(|r| r.workspace == workspace && r.requester == person).collect()
    }
}

/// Whether a name is fit to be a file in a volume.
pub(crate) fn fit_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0') && name.len() <= MAX_FILE_NAME_BYTES
}

const MAX_FILE_NAME_BYTES: usize = 200;

#[cfg(test)]
mod tests {
    use super::*;

    fn made() -> Ledger {
        let mut l = Ledger::default();
        l.apply(&Change::Created { workspace: "w".into(), name: "Team".into(), owner: "alice".into() });
        l
    }

    #[test]
    fn the_creator_owns_it() {
        assert_eq!(made().role("w", "alice"), Some(Role::Owner));
        assert_eq!(made().role("w", "bob"), None);
    }

    #[test]
    fn roles_are_ordered_from_the_least() {
        assert!(Role::Guest < Role::Member && Role::Member < Role::Admin && Role::Admin < Role::Owner);
    }

    #[test]
    fn removing_a_member_forgets_what_was_held_for_them() {
        let mut l = made();
        l.apply(&Change::Member { workspace: "w".into(), person: "bob".into(), role: Role::Member });
        l.apply(&Change::Held { workspace: "w".into(), person: "bob".into(), file: File { name: "f".into(), size: 1, by: "bob".into() } });
        l.apply(&Change::Removed { workspace: "w".into(), person: "bob".into() });
        assert_eq!(l.role("w", "bob"), None);
        assert!(l.workspace("w").unwrap().held.is_empty());
    }

    #[test]
    fn an_empty_grant_takes_the_app_away() {
        let mut l = made();
        l.apply(&Change::Connector { workspace: "w".into(), app: "crm".into(), roles: vec![Role::Admin] });
        assert!(l.workspace("w").unwrap().connectors.contains_key("crm"));
        l.apply(&Change::Connector { workspace: "w".into(), app: "crm".into(), roles: vec![] });
        assert!(l.workspace("w").unwrap().connectors.is_empty());
    }

    #[test]
    fn the_strictest_workspace_sets_the_ceiling() {
        use crate::local::Mode;
        let mut l = made();
        l.apply(&Change::Created { workspace: "x".into(), name: "Other".into(), owner: "alice".into() });
        l.apply(&Change::LocalCeiling { workspace: "w".into(), mode: Mode::Allow });
        l.apply(&Change::LocalCeiling { workspace: "x".into(), mode: Mode::Ask });
        assert_eq!(l.local_ceiling("alice"), Some(Mode::Ask));
        assert_eq!(l.local_ceiling("bob"), None);
    }

    #[test]
    fn a_retired_approvers_rule_is_read_and_changes_nothing() {
        let mut l = made();
        l.apply(&Change::Member { workspace: "w".into(), person: "bob".into(), role: Role::Member });
        let before = serde_json::to_value(l.workspace("w").unwrap()).unwrap();
        let old: Change = serde_json::from_value(serde_json::json!({ "change": "approvers", "workspace": "w", "resource": "crm", "people": ["bob"] })).unwrap();
        l.apply(&old);
        assert_eq!(serde_json::to_value(l.workspace("w").unwrap()).unwrap(), before, "an old approvers rule still names who answers");
    }

    #[test]
    fn a_retired_fact_reads_as_an_entry() {
        let mut l = made();
        l.apply(&Change::Fact { workspace: "w".into(), id: "f".into(), text: "We use Rust".into(), by: "alice".into() });
        assert_eq!(l.workspace("w").unwrap().entries(None)[0].statement, "We use Rust");
        l.apply(&Change::FactRemoved { workspace: "w".into(), id: "f".into() });
        assert!(l.workspace("w").unwrap().entries(None).is_empty());
    }

    #[test]
    fn a_file_name_cannot_climb_out() {
        for bad in ["", ".", "..", "a/b", "../x"] {
            assert!(!fit_name(bad), "{bad}");
        }
        assert!(fit_name("report.pdf"));
    }
}
