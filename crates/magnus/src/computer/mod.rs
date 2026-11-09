//! Computers: a person's or workspace's own VM (spec, the agent computer).
//! Magnus decides who may do what and records it; `magnus-computerd` is the
//! only thing that touches a VM (`decisions/2026-11-08-computerd.md`).

mod client;
mod routes;
mod service;

pub use routes::router;
pub use service::schedule;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// `[computer]`: where computerd listens.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub socket: PathBuf,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Owner {
    Person(String),
    Workspace(String),
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KillMode {
    Pause,
    Destroy,
}

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
    Started {
        id: String,
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
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Stopped,
    Queued,
    Running,
    Paused,
    Killed,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub id: String,
    pub label: String,
    pub revoked: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Computer {
    pub id: String,
    pub owner: Owner,
    pub status: Status,
    pub grants: Vec<Grant>,
    /// Its place among waiting wakes, oldest first.
    #[serde(skip)]
    queued: Option<u64>,
}

/// The log's computers, rebuilt by replay.
#[derive(Default, Clone, Debug)]
pub struct Ledger {
    computers: BTreeMap<String, Computer>,
    next_in_queue: u64,
}

impl Ledger {
    pub fn get(&self, id: &str) -> Option<&Computer> {
        self.computers.get(id)
    }

    pub fn all(&self) -> impl Iterator<Item = &Computer> {
        self.computers.values()
    }

    /// Waiting wakes, oldest first.
    pub fn queue(&self) -> Vec<&Computer> {
        let mut waiting: Vec<&Computer> = self.computers.values().filter(|c| c.status == Status::Queued).collect();
        waiting.sort_by_key(|c| c.queued);
        waiting
    }

    pub fn apply(&mut self, change: &Change) {
        match change {
            Change::Created { id, owner } => {
                self.computers.insert(id.clone(), Computer { id: id.clone(), owner: owner.clone(), status: Status::Stopped, grants: vec![], queued: None });
            }
            Change::Granted { id, grant, label } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.grants.push(Grant { id: grant.clone(), label: label.clone(), revoked: false });
                }
            }
            Change::Queued { id } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.status = Status::Queued;
                    c.queued = Some(self.next_in_queue);
                    self.next_in_queue += 1;
                }
            }
            Change::Started { id } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.status = Status::Running;
                    c.queued = None;
                }
            }
            Change::Killed { id, mode, delete_volume } => {
                if *delete_volume {
                    self.computers.remove(id);
                } else if let Some(c) = self.computers.get_mut(id) {
                    c.status = if *mode == KillMode::Pause { Status::Paused } else { Status::Killed };
                    c.queued = None;
                }
            }
            Change::Revoked { id, grant } => {
                if let Some(g) = self.computers.get_mut(id).and_then(|c| c.grants.iter_mut().find(|g| g.id == *grant)) {
                    g.revoked = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn created(l: &mut Ledger, id: &str) {
        l.apply(&Change::Created { id: id.into(), owner: Owner::Person("alice".into()) });
    }

    #[test]
    fn a_change_survives_the_log_in_its_own_shape() {
        let ev = crate::events::Ev::Computer(Change::Killed { id: "a".into(), mode: KillMode::Destroy, delete_volume: false });
        let text = serde_json::to_value(&ev).unwrap();
        assert_eq!(text["kind"], "computer");
        assert_eq!(text["change"], "killed");
        let back: crate::events::Ev = serde_json::from_value(text).unwrap();
        assert!(matches!(back, crate::events::Ev::Computer(Change::Killed { mode: KillMode::Destroy, .. })));
    }

    #[test]
    fn a_wake_waits_its_turn_and_a_kill_takes_it_out_of_the_queue() {
        let mut l = Ledger::default();
        for id in ["b", "a", "c"] {
            created(&mut l, id);
        }
        for id in ["b", "a", "c"] {
            l.apply(&Change::Queued { id: id.into() });
        }
        assert_eq!(l.queue().iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["b", "a", "c"]);
        l.apply(&Change::Killed { id: "a".into(), mode: KillMode::Destroy, delete_volume: false });
        assert_eq!(l.queue().iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["b", "c"]);
    }

    #[test]
    fn a_rebuild_does_not_give_back_a_revoked_grant() {
        let mut l = Ledger::default();
        created(&mut l, "a");
        l.apply(&Change::Granted { id: "a".into(), grant: "g".into(), label: "x".into() });
        l.apply(&Change::Killed { id: "a".into(), mode: KillMode::Destroy, delete_volume: false });
        l.apply(&Change::Revoked { id: "a".into(), grant: "g".into() });
        l.apply(&Change::Started { id: "a".into() });
        let c = l.get("a").unwrap();
        assert_eq!(c.status, Status::Running);
        assert!(c.grants[0].revoked);
    }

    #[test]
    fn deleting_the_volume_forgets_the_computer() {
        let mut l = Ledger::default();
        created(&mut l, "a");
        l.apply(&Change::Killed { id: "a".into(), mode: KillMode::Destroy, delete_volume: true });
        assert!(l.get("a").is_none());
    }
}
