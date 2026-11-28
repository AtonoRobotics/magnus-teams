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

#[cfg(test)]
mod access_tests;
#[cfg(test)]
mod admission_tests;
mod agent;
mod caller;
mod change;
pub mod chat;
mod chat_routes;
pub(crate) mod coedit;
pub mod conversation;
pub(crate) mod copies;
#[cfg(test)]
mod copies_tests;
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
#[cfg(test)]
mod lane_knowledge_tests;
pub(crate) mod lane_release;
#[cfg(test)]
pub(crate) mod lane_release_tests;
pub mod lanes;
#[cfg(test)]
mod lanes_tests;
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
pub mod mission_routes;
pub mod missions;
mod notices;
pub mod orchestrator;
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
mod promote;
#[cfg(test)]
mod promote_tests;
#[cfg(test)]
mod recorded_run;
mod reply;
mod routes;
mod routine_sweep;
#[cfg(test)]
mod routine_tests;
pub(crate) mod routines;
mod search;
pub(crate) mod sign_in;
pub mod spend;
#[cfg(test)]
mod spend_tests;
#[cfg(test)]
mod talk_tests;
pub mod team_admin;
#[cfg(test)]
mod trust_tests;
pub(crate) mod upkeep;
mod whisper;

pub(crate) use agent::refusal;
pub(crate) use agent::signs_in_per_person;
pub(crate) use agent::team_specs as specs;
pub(crate) use agent::{TeamOp, BUILTINS};
pub use change::{Change, File, Message, Note, TeamRun};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions_by: Option<String>,
    #[serde(skip_serializing_if = "coedit::Coedits::is_empty")]
    pub coedit: coedit::Coedits,
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
    /// Where messages are read again, or were copied (`copies`).
    copies: copies::TeamCopies,
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
        self.copies.note(c, seq);
        if let Change::Coedit { workspace, coedit, .. } = c {
            self.workspaces.get_mut(workspace).into_iter().for_each(|w| coedit::apply(w, coedit, seq));
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
        if let Change::Whisper { workspace, channel, thread, task, owner, .. } = c {
            let w = Whisper { workspace: workspace.clone(), channel: channel.clone(), thread: thread.clone(), task: task.clone(), owner: owner.clone(), shared: false };
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
                    routines::apply(w, c);
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
                    routines::apply(w, c);
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
                    coedit::on_file(w, file);
                }
            }
            Change::FileRemoved { workspace, name } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.files.remove(name);
                    coedit::on_removed(w, name);
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
            // Deleted with the person who owned it: its channels go with it,
            // never left open to the workspace, and what else names it is
            // closed to everyone (`Workspace::may_read`).
            Change::ProjectRemoved { workspace, id } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.projects.remove(id);
                    let gone: Vec<String> = w.channels.iter().filter(|(_, c)| c.project.as_deref() == Some(id.as_str())).map(|(c, _)| c.clone()).collect();
                    chat::drop_channels(w, &gone);
                }
            }
            Change::ChannelProject { workspace, channel, project } => {
                if let Some(ch) = self.workspaces.get_mut(workspace).and_then(|w| w.channels.get_mut(channel)) {
                    ch.project = project.clone();
                }
            }
            Change::Routine { workspace, .. }
            | Change::RoutineRemoved { workspace, .. }
            | Change::RoutineRan { workspace, .. }
            | Change::RoutineFailed { workspace, .. }
            | Change::RoutineSkipped { workspace, .. }
            | Change::RoutineStopped { workspace, .. } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    routines::apply(w, c);
                }
            }
            Change::Renamed { workspace, name } => {
                // A deleted name reads empty: the name before it stands.
                if let Some(w) = self.workspaces.get_mut(workspace).filter(|_| !name.is_empty()) {
                    w.name = name.clone();
                }
            }
            Change::Instructions { workspace, text, by } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.instructions = text.clone();
                    w.instructions_by = text.as_ref().map(|_| by.clone());
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
            | Change::TeamAdminsSeeded { .. }
            | Change::Coedit { .. } => {}
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

    /// A person's whispers anywhere in the workspace, whichever channel.
    pub fn whispers_owned_by(&self, workspace: &str, person: &str) -> Vec<&Whisper> {
        self.whispers.values().filter(|w| w.workspace == workspace && w.owner == person).collect()
    }

    pub fn whispers_of(&self, workspace: &str) -> Vec<&Whisper> {
        self.whispers.values().filter(|w| w.workspace == workspace).collect()
    }

    pub fn runs_by(&self, workspace: &str, person: &str) -> Vec<&TeamRun> {
        self.runs.values().filter(|r| r.workspace == workspace && r.requester == person).collect()
    }
}

/// Whether a name is fit to be a file in a volume.
/// Where a read of a shared project's own knowledge or files says its
/// private data is from, in the result's data (`boundary::finish`).
pub(crate) const SOURCE_KEY: &str = "private_source";
const PROJECT_SOURCE_PREFIX: &str = "team:";

/// A shared project, as the source of private data a run read from it. A
/// post back into that same project leaks none of it
/// (`lane_release::holds_private_for`).
pub(crate) fn project_source(workspace: &str) -> String {
    format!("{PROJECT_SOURCE_PREFIX}{workspace}")
}

/// Whether a run's private source is a shared project's own.
pub(crate) fn is_project_source(source: &str) -> bool {
    source.starts_with(PROJECT_SOURCE_PREFIX)
}

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
    fn shared_project_instructions_apply_and_clear() {
        let mut l = made();
        l.apply(&Change::Instructions { workspace: "w".into(), text: Some("Be concise.".into()), by: "alice".into() });
        let w = l.workspace("w").unwrap();
        assert_eq!(w.instructions.as_deref(), Some("Be concise."));
        assert_eq!(w.instructions_by.as_deref(), Some("alice"));
        l.apply(&Change::Instructions { workspace: "w".into(), text: None, by: "bob".into() });
        let w = l.workspace("w").unwrap();
        assert_eq!(w.instructions, None);
        assert_eq!(w.instructions_by, None);
    }

    #[test]
    fn removing_a_member_forgets_what_was_held_for_them() {
        let mut l = made();
        l.apply(&Change::Member { workspace: "w".into(), person: "bob".into(), role: Role::Member });
        l.apply(&Change::Held { workspace: "w".into(), person: "bob".into(), file: File { name: "f".into(), size: 1, by: "bob".into(), coedit: false } });
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
