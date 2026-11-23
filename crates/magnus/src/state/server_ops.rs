//! The server's own upkeep: its updates and rollbacks, and its backups'
//! checks, off-host copies and scrubs (K10, #195, #297).

use crate::events::Ev;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize)]
pub struct Update {
    /// "updated", "rolled_back" or "failed".
    pub what: String,
    pub from: String,
    pub to: String,
    pub detail: String,
    pub at: DateTime<Utc>,
}

/// Removed content still in backups kept, while it is (#195): the backups
/// whose rewrite failed have had the removals up to `from`, not those after
/// it through `through`.
#[derive(Clone, Debug, Serialize)]
pub struct ScrubPending {
    pub from: i64,
    pub through: i64,
    pub files: Vec<String>,
    pub detail: String,
    pub at: DateTime<Utc>,
}

/// A backup's check or copy: file, whether it held, detail, when.
pub type BackupResult = (String, bool, String, DateTime<Utc>);

#[derive(Default, Debug)]
pub struct ServerOps {
    /// An update in progress: from, to, and whether it is a rollback.
    updating: Option<(String, String, bool)>,
    last_update: Option<Update>,
    /// Releases rolled back from: not installed again by themselves.
    skipped_releases: BTreeSet<String>,
    /// The latest backup's restore check.
    last_backup: Option<BackupResult>,
    /// The latest backup's off-host copy.
    last_copy: Option<BackupResult>,
    /// Removed content some backups still hold, until they are rewritten.
    scrub_pending: Option<ScrubPending>,
}

/// The server's condition is nobody's.
impl crate::forget::Forget for ServerOps {
    fn forget(&mut self, _: &crate::forget::Erased) {}
}

impl ServerOps {
    /// The update, backup and scrub kinds. `State::apply` routes no other
    /// kind here.
    pub(crate) fn fold(&mut self, ev: &Ev, at: DateTime<Utc>) {
        match ev {
            Ev::BackupMade { .. } => {}
            Ev::UpdateStarted { from, to, rollback } => self.updating = Some((from.clone(), to.clone(), *rollback)),
            Ev::UpdateFinished { from, to } => {
                self.updating = None;
                self.last_update = Some(Update { what: "updated".into(), from: from.clone(), to: to.clone(), detail: String::new(), at });
            }
            Ev::UpdateRolledBack { from, to, reason } => {
                self.updating = None;
                self.skipped_releases.insert(from.clone());
                self.last_update = Some(Update { what: "rolled_back".into(), from: from.clone(), to: to.clone(), detail: reason.clone(), at });
            }
            Ev::UpdateFailed { to, reason } => {
                self.updating = None;
                self.last_update = Some(Update { what: "failed".into(), from: String::new(), to: to.clone(), detail: reason.clone(), at });
            }
            Ev::BackupChecked { file, ok, detail } => {
                self.last_backup = Some((file.clone(), *ok, detail.clone(), at));
            }
            Ev::BackupCopied { file, ok, detail } => {
                self.last_copy = Some((file.clone(), *ok, detail.clone(), at));
            }
            Ev::BackupScrubPending { from, through, files, detail } => {
                self.scrub_pending = Some(ScrubPending { from: *from, through: *through, files: files.clone(), detail: detail.clone(), at });
            }
            Ev::BackupsScrubbed { through } if self.scrub_pending.as_ref().is_some_and(|p| p.through <= *through) => {
                self.scrub_pending = None;
            }
            _ => {}
        }
    }

    /// The update in progress: from, to, and whether it is a rollback.
    pub fn updating(&self) -> Option<&(String, String, bool)> {
        self.updating.as_ref()
    }

    pub fn last_update(&self) -> Option<&Update> {
        self.last_update.as_ref()
    }

    /// Whether the release was rolled back from, so not installed again by itself.
    pub fn skipped(&self, release: &str) -> bool {
        self.skipped_releases.contains(release)
    }

    pub fn skipped_releases(&self) -> &BTreeSet<String> {
        &self.skipped_releases
    }

    pub fn last_backup(&self) -> Option<&BackupResult> {
        self.last_backup.as_ref()
    }

    pub fn last_copy(&self) -> Option<&BackupResult> {
        self.last_copy.as_ref()
    }

    pub fn scrub_pending(&self) -> Option<&ScrubPending> {
        self.scrub_pending.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn canonical_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("updating", format!("{:?}", self.updating)),
            ("last_update", format!("{:?}", self.last_update)),
            ("skipped_releases", format!("{:?}", self.skipped_releases)),
            ("last_backup", format!("{:?}", self.last_backup)),
            ("last_copy", format!("{:?}", self.last_copy)),
            ("scrub_pending", format!("{:?}", self.scrub_pending)),
        ]
    }
}
