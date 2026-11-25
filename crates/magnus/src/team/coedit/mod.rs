//! Co-editing shared files (the owner's design, Co-editing shared files;
//! decisions/2026-11-23-co-editing-shared-files.md). A member sharing a
//! file picks "Share a copy", which stays theirs as before (`keep`), or
//! "Co-edit": the file then belongs to the project and changes only by
//! versions. The sharer keeps their own copy in their library as it was.
//!
//! Every change is a new version recorded with its base version, its author
//! (a person, or a person's agent and its run) and what it changed. Its
//! bytes are its event's content, kept beside the log (invariant 3), so the
//! history is whole and a revert is one more version. A change made on a
//! stale base merges by format (`merge`); one that conflicts is rejected and
//! kept as its author's, with both versions of each conflicting part, so
//! nothing is lost. An agent's edit is a proposal that waits with its lane's
//! result and lands only when that result passes the release gate
//! (`lane_release`). A binary file is checked out to one editor at a time,
//! under a lease that expires.
//!
//! A version's author is in its content and nowhere in its record, and no
//! version is anyone's own content: erasing a member keeps what they wrote
//! and names "a former member" in their place (owner, 2026-11-23; `commit`).

mod commit;
pub(crate) mod merge;
mod records;
pub(super) mod routes;
mod sheet;
#[cfg(test)]
mod tests;
mod text;
mod tool;

pub(super) use commit::share_lock;
pub use commit::Landed;
pub use merge::{Conflict, Edit, Format};
pub(crate) use tool::versions_sent;

use super::{File, Workspace};
use crate::forget::Erased;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

/// Who an erased member's versions name instead (owner, 2026-11-23).
pub const FORMER_MEMBER: &str = "a former member";
/// How long a check-out holds a binary file for its editor: a working day.
/// Checking it out again renews it.
pub const CHECKOUT_LEASE_SECS: Duration = Duration::from_secs(8 * 60 * 60);

/// Who made a change: a person, or a person's agent in one of its runs.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct Author {
    pub person: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

/// A version of a co-edited file, as its event's content holds it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Version {
    pub name: String,
    pub number: u32,
    /// The version the change was made on; 0 for the share.
    pub base: u32,
    pub author: Author,
    pub at: DateTime<Utc>,
    pub data_b64: String,
    /// What the change made of its base.
    #[serde(default)]
    pub diff: Vec<Edit>,
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
}

/// A change kept as its author's instead of landing: an agent's edit that
/// waits for its lane's release, or a change that conflicted.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Proposal {
    pub id: String,
    pub name: String,
    pub base: u32,
    pub author: Author,
    pub at: DateTime<Utc>,
    pub data_b64: String,
    #[serde(default)]
    pub diff: Vec<Edit>,
    /// The lane whose result it goes with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    /// What conflicted when it was last tried: both versions of each part.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<Conflict>,
}

/// One step of a co-edited file's history: the content of a `Coedit` change.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Step {
    /// Its content was deleted: it reads as nothing.
    #[default]
    Gone,
    Version(Version),
    Proposed(Proposal),
    /// A proposal landed as `version`, or was thrown away (none).
    Settled {
        id: String,
        #[serde(default)]
        version: Option<u32>,
    },
    CheckedOut {
        name: String,
        person: String,
        until: DateTime<Utc>,
    },
    CheckedIn {
        name: String,
        person: String,
    },
    /// An erased member's version written again, "a former member" its
    /// author: it stands where theirs did.
    Reattributed(Version),
}

impl Step {
    /// Whose content it is: a proposal its author's, a check-out its
    /// holder's. A version is no one's: it is the project's.
    pub fn owner(&self) -> Option<&str> {
        match self {
            Step::Proposed(p) => Some(&p.author.person),
            Step::CheckedOut { person, .. } | Step::CheckedIn { person, .. } => Some(person),
            Step::Gone | Step::Version(_) | Step::Settled { .. } | Step::Reattributed(_) => None,
        }
    }
}

/// A version, as the state knows it: its bytes are its event's content.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Meta {
    pub number: u32,
    pub base: u32,
    pub author: Author,
    pub at: DateTime<Utc>,
    pub sha256: String,
    /// The event whose content holds it.
    #[serde(skip)]
    pub seq: i64,
}

/// An open proposal, as the state knows it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Open {
    pub id: String,
    pub name: String,
    pub base: u32,
    pub author: Author,
    pub at: DateTime<Utc>,
    pub mission: Option<String>,
    pub lane: Option<String>,
    pub conflicts: Vec<Conflict>,
    pub diff: Vec<Edit>,
    #[serde(skip)]
    pub seq: i64,
}

/// Who has a binary file checked out, and until when.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Lease {
    pub person: String,
    pub until: DateTime<Utc>,
}

/// A co-edited file.
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Coedited {
    pub format: Format,
    pub versions: BTreeMap<u32, Meta>,
    pub lease: Option<Lease>,
}

impl Coedited {
    pub fn head(&self) -> Option<&Meta> {
        self.versions.values().next_back()
    }

    /// Its lease, while it holds.
    pub fn held(&self, now: DateTime<Utc>) -> Option<&Lease> {
        self.lease.as_ref().filter(|l| l.until > now)
    }

    /// Everyone who wrote any of it.
    pub fn writers(&self) -> impl Iterator<Item = &str> {
        self.versions.values().map(|m| m.author.person.as_str())
    }
}

/// A workspace's co-edited files and the proposals still open on them.
#[derive(Serialize, Clone, Debug, Default)]
pub struct Coedits {
    pub files: BTreeMap<String, Coedited>,
    pub open: BTreeMap<String, Open>,
}

impl Coedits {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.open.is_empty()
    }

    /// The proposals waiting with a lane's result.
    pub fn of_lane(&self, mission: &str, lane: &str) -> Vec<&Open> {
        self.open.values().filter(|o| o.mission.as_deref() == Some(mission) && o.lane.as_deref() == Some(lane)).collect()
    }

    /// A person's open proposals: their own and their agent's.
    pub fn of_person<'a>(&'a self, person: &'a str) -> impl Iterator<Item = &'a Open> + 'a {
        self.open.values().filter(move |o| o.author.person == person)
    }
}

/// What an edit changes, as the release gate's checks read it: each part as
/// it will be.
pub(crate) fn shown(diff: &[Edit]) -> String {
    diff.iter().map(|e| format!("{}: {}\n", e.at, e.after.as_deref().unwrap_or("(removed)"))).collect()
}

/// A co-edited file as the files list shows it: its version, how it merges,
/// who has it checked out, and how many changes of `person`'s wait on it.
pub(crate) fn listed(w: &Workspace, name: &str, person: &str) -> serde_json::Value {
    let Some(f) = w.coedit.files.get(name) else { return serde_json::Value::Null };
    let lease = f.held(Utc::now()).map(|l| serde_json::json!({ "person": l.person, "until": l.until }));
    let open = w.coedit.of_person(person).filter(|o| o.name == name).count();
    serde_json::json!({ "version": f.head().map_or(0, |h| h.number), "format": f.format, "checked_out": lease, "open_changes": open })
}

/// Each of `viewer`'s own lanes in a room's missions, with the edits that
/// wait with its result: only its owner sees them, as only they see its draft.
pub(crate) fn with_edits(view: &mut [serde_json::Value], w: &Workspace, viewer: &str) {
    for m in view.iter_mut() {
        let mission = m["id"].as_str().unwrap_or_default().to_string();
        let Some(lanes) = m["lanes"].as_array_mut() else { continue };
        for l in lanes.iter_mut().filter(|l| l["owner"] == viewer) {
            let edits: Vec<serde_json::Value> = w.coedit.of_lane(&mission, l["id"].as_str().unwrap_or_default()).into_iter().map(|o| serde_json::json!(o)).collect();
            if !edits.is_empty() {
                l["edits"] = serde_json::Value::Array(edits);
            }
        }
    }
}

/// A shared file came in: a co-edited one starts its history.
pub(super) fn on_file(w: &mut Workspace, f: &File) {
    if f.coedit {
        w.coedit.files.insert(f.name.clone(), Coedited { format: Format::of(&f.name), ..Default::default() });
    } else {
        w.coedit.files.remove(&f.name);
    }
}

/// The file left the volume: its history in the state goes with it (its
/// versions stay in the log).
pub(super) fn on_removed(w: &mut Workspace, name: &str) {
    w.coedit.files.remove(name);
    w.coedit.open.retain(|_, o| o.name != name);
}

fn meta(v: &Version, seq: i64) -> Meta {
    Meta { number: v.number, base: v.base, author: v.author.clone(), at: v.at, sha256: v.sha256.clone(), seq }
}

/// Apply one step, from the event at `seq`.
pub(super) fn apply(w: &mut Workspace, step: &Step, seq: i64) {
    let c = &mut w.coedit;
    match step {
        Step::Gone => {}
        Step::Version(v) | Step::Reattributed(v) => {
            if let Some(f) = c.files.get_mut(&v.name) {
                f.versions.insert(v.number, meta(v, seq));
                // The file's size is its head's: an old version written
                // again (an erasure's) leaves it as it is.
                let head = f.head().is_some_and(|h| h.number == v.number);
                if let Some(file) = w.files.get_mut(&v.name).filter(|_| head) {
                    file.size = v.size;
                }
            }
        }
        Step::Proposed(p) => {
            let open = Open {
                id: p.id.clone(),
                name: p.name.clone(),
                base: p.base,
                author: p.author.clone(),
                at: p.at,
                mission: p.mission.clone(),
                lane: p.lane.clone(),
                conflicts: p.conflicts.clone(),
                diff: p.diff.clone(),
                seq,
            };
            c.open.insert(p.id.clone(), open);
        }
        Step::Settled { id, .. } => {
            c.open.remove(id);
        }
        Step::CheckedOut { name, person, until } => {
            if let Some(f) = c.files.get_mut(name) {
                f.lease = Some(Lease { person: person.clone(), until: *until });
            }
        }
        Step::CheckedIn { name, person } => {
            if let Some(f) = c.files.get_mut(name).filter(|f| f.lease.as_ref().is_some_and(|l| &l.person == person)) {
                f.lease = None;
            }
        }
    }
}

/// Erasing a member: their proposals and check-outs go; any version still
/// naming them names "a former member" (`Kernel::coedit_reattribute` wrote
/// each of theirs again before this).
pub(super) fn forget(w: &mut Workspace, who: &Erased) {
    let c = &mut w.coedit;
    c.open.retain(|_, o| !who.is(&o.author.person));
    for f in c.files.values_mut() {
        if f.lease.as_ref().is_some_and(|l| who.is(&l.person)) {
            f.lease = None;
        }
        for m in f.versions.values_mut().filter(|m| who.is(&m.author.person)) {
            m.author = Author { person: FORMER_MEMBER.into(), run: None };
        }
    }
}
