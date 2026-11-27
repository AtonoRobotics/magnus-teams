//! Computer event log change variants (split from `computer/mod.rs`).

use super::{App, KillMode, Owner, Role, Theme};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// What happened to a computer, as the log keeps it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    Created {
        id: String,
        owner: Owner,
    },
    Granted {
        id: String,
        grant: String,
        label: String,
    },
    Queued {
        id: String,
    },
    /// A wake from the queue failed for a reason other than room: it leaves
    /// the queue, and its next use tries again and says why.
    WakeFailed {
        id: String,
        error: String,
    },
    Started {
        id: String,
    },
    BrowserCreated {
        id: String,
        owner: Owner,
        role: Role,
    },
    /// The VM stopped, and an identity's profile is being sealed.
    BrowserStopped {
        id: String,
    },
    /// A secure form filled a page. The value is never written anywhere.
    SecureFormFilled {
        id: String,
        by: String,
        saved: bool,
        at: DateTime<Utc>,
    },
    /// The hash only: the token itself is held by the computer.
    SurrogateIssued {
        id: String,
        hash: String,
        name: String,
        host: String,
    },
    /// The surrogate stops standing in for anything: replaced by a newer
    /// credential of the same name.
    SurrogateRevoked {
        id: String,
        hash: String,
    },
    /// One step with the `Revoked` of each of its grants, in the same write.
    Killed {
        id: String,
        mode: KillMode,
        delete_volume: bool,
    },
    Revoked {
        id: String,
        grant: String,
    },
    Hibernated {
        id: String,
    },
    /// The machine found it no longer running though Magnus had it running
    /// (its host restarted, or its VM ended without Magnus): it is stopped,
    /// or asleep when a snapshot was kept, and the next use wakes it.
    FoundStopped {
        id: String,
        hibernated: bool,
    },
    /// Reachable by this computer from now on; a boundary action.
    DestinationAdded {
        id: String,
        host: String,
    },
    /// Reachable by this computer while one task is live; the person
    /// granted the source to that task alone.
    DestinationGranted {
        id: String,
        host: String,
        task: String,
    },
    PageOpened {
        id: String,
        page: String,
        host: String,
    },
    PageClosed {
        id: String,
        page: String,
    },
    /// Written before an install counts as done, and on an update of it.
    AppInstalled {
        id: String,
        app: App,
    },
    AppRemoved {
        id: String,
        name: String,
    },
    /// The manifest could not be replayed onto a new overlay.
    ReplayFailed {
        id: String,
        app: String,
        error: String,
    },
    /// The path only: nothing of the file's content is kept here.
    FileDeleted {
        id: String,
        path: String,
        at: DateTime<Utc>,
    },
    /// Free space was zeroed, so earlier versions of deleted files are gone.
    FreeSpaceZeroed {
        id: String,
        at: DateTime<Utc>,
    },
    /// The volume was put back from a backup taken at `at`.
    Restored {
        id: String,
        at: u64,
    },
    /// One person holds the screen; the agent is paused on it.
    TakeoverStarted {
        id: String,
        by: String,
        at: DateTime<Utc>,
        /// Why, as far as the log can tell: what the agent was waiting on,
        /// or that the person stepped in. Absent from logs before it.
        #[serde(default)]
        cause: Option<String>,
        /// The runs using the computer then.
        #[serde(default)]
        runs: Vec<String>,
    },
    TakeoverEnded {
        id: String,
        by: String,
        at: DateTime<Utc>,
    },
    /// At handback the guest stopped what the takeover left running and
    /// undid the autostart entries it made, named here; written with the
    /// `TakeoverEnded` it allowed (computer/leftovers.rs).
    TakeoverCleared {
        id: String,
        by: String,
        stopped: Vec<String>,
        stopped_count: u64,
        removed: Vec<String>,
        at: DateTime<Utc>,
    },
    /// The guest could not confirm that: the person keeps the screen and
    /// the agent stays paused.
    TakeoverUnclean {
        id: String,
        by: String,
        why: String,
        at: DateTime<Utc>,
    },
    /// The computer's user let an operator look, until `until`.
    OperatorGranted {
        id: String,
        session: String,
        operator: String,
        until: DateTime<Utc>,
        at: DateTime<Utc>,
    },
    OperatorEnded {
        id: String,
        session: String,
        at: DateTime<Utc>,
    },
    /// Written before an operator is shown anything.
    OperatorLooked {
        id: String,
        session: String,
        operator: String,
        what: String,
        at: DateTime<Utc>,
    },
    /// An owner had several computers and keeps this one, the one used
    /// last (spec rev 142, One computer, always there). The others are
    /// deleted by their own `Killed` in the same write.
    Kept {
        id: String,
        owner: Owner,
        removed: Vec<String>,
    },
    /// A reset that erased the person's files: the same computer, rebuilt
    /// from the base image and its manifest, with a new, empty volume.
    VolumeErased {
        id: String,
    },
    /// A kill switch paused it (its `Killed` with `Pause` in the same
    /// write); releasing that switch, `reason`, resumes it.
    HeldBySwitch {
        id: String,
        reason: String,
    },
    /// The look of its desktop, following its person's app theme (spec
    /// rev 145, The desktop).
    DesktopTheme {
        id: String,
        theme: Theme,
    },
}
