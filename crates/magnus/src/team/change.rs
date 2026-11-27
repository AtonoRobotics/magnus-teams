//! Team workspace log change variants and payload types.

use super::knowledge::{Entry, Project};
use super::routines::Routine;
use super::{chat, coedit, Role};
use serde::{Deserialize, Serialize};

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
    /// A routine's firing could not start its run, and why: its admin is told (`routines`).
    RoutineFailed {
        workspace: String,
        id: String,
        error: String,
        at: chrono::DateTime<chrono::Utc>,
    },
    /// A routine's firing started nothing, because its last run was still open.
    RoutineSkipped {
        workspace: String,
        id: String,
        task: String,
        reason: String,
        at: chrono::DateTime<chrono::Utc>,
    },
    /// Too many of a routine's firings in a row failed: it starts no more until it is changed or removed.
    RoutineStopped {
        workspace: String,
        id: String,
        reason: String,
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
    /// Shared project instructions (mirrors personal Project::instructions).
    Instructions {
        workspace: String,
        #[serde(default)]
        text: Option<String>,
        by: String,
    },
    /// A co-edited file's history: see `coedit`. Everything but the
    /// workspace is content, its people included.
    Coedit {
        workspace: String,
        #[serde(default)]
        coedit: coedit::Step,
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
    /// The member it was made for or by; no one for a co-edited file,
    /// which is the project's.
    pub by: String,
    /// Shared to co-edit (`coedit`), not as a copy.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub coedit: bool,
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
