//! Making and deleting computers, so the log and computerd agree
//! (`decisions/2026-11-23-computers-agree-with-computerd.md`).
//!
//! A computer is made by computerd first, then recorded; one the log cannot
//! record is deleted again. A deletion is recorded first: the log forgets
//! the computer and keeps its id among those computerd still owes, then
//! computerd is asked. A deletion computerd does not finish now (it was
//! restarting, say) is finished at the next agreement: every minute the
//! list computerd keeps is compared with the log, and every computer it
//! still has that the log deleted goes. Computerd is never asked to delete
//! a computer the log did not make and delete.

use super::client::{self, Request};
use super::{Change, Status};
use crate::controls::Refused;
use crate::events::Ev;
use crate::kernel::Kernel;
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

/// Computers computerd made that the log could not record, and that it did
/// not delete again at once.
static UNRECORDED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
/// Computers the log has that computerd does not, already said once.
static MISSING_SAID: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

impl super::Ledger {
    /// Every computer the log deleted. Computerd must keep none of them.
    pub(super) fn deleted(&self) -> &BTreeSet<String> {
        &self.deleted
    }
}

impl Kernel {
    /// Computerd makes `id`, then the log records `changes`, the first of
    /// which makes it in the log.
    pub(super) async fn make_computer(&self, actor: &str, id: &str, changes: Vec<Change>) -> Result<()> {
        let socket = self.computer_socket().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        client::ask(&socket, Request::Create { id: id.to_string() }).await?;
        if let Err(e) = self.emit_all(changes.into_iter().map(|c| (actor, Ev::Computer(c))).collect()).await {
            if client::ask(&socket, Request::Destroy { id: id.to_string(), delete_volume: true }).await.is_err() {
                UNRECORDED.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_string());
            }
            return Err(e);
        }
        Ok(())
    }

    /// Deletes computers: `evs` (with each computer's `Killed` with its
    /// volume, last of its own) are recorded in one write, what is kept here
    /// for each goes at once, then computerd deletes them, now or at the next
    /// agreement.
    pub(super) async fn delete_computers(&self, evs: Vec<(&str, Ev)>) -> Result<(), Refused> {
        let socket = self.computer_socket()?;
        let ids: Vec<String> = evs
            .iter()
            .filter_map(|(_, e)| match e {
                Ev::Computer(Change::Killed { id, delete_volume: true, .. }) => Some(id.clone()),
                _ => None,
            })
            .collect();
        self.emit_all(evs).await.map_err(super::service::failed)?;
        for id in &ids {
            self.forget_here(id);
            if let Err(e) = client::ask(&socket, Request::Destroy { id: id.clone(), delete_volume: true }).await {
                tracing::warn!("computer {id} is deleted; the computer service deletes it at the next look: {e:#}");
            }
        }
        Ok(())
    }

    /// What Magnus keeps of a deleted computer besides the log: its
    /// secrets, its last screen and what its proxy carried.
    fn forget_here(&self, id: &str) {
        if let Err(e) = super::lifecycle::forget_secrets(&self.vault, id) {
            tracing::warn!("computer {id}'s secrets were not removed yet; the sweep at start removes them: {e:#}");
        }
        super::wake::forget_screen(id);
        super::egress::Carried::at(&self.cfg.appliance.state_dir).forget(id);
    }

    /// Computerd's list against the log: every computer the log deleted
    /// that computerd still has is deleted now.
    pub(crate) async fn agree_with_computerd(&self) {
        let Ok(socket) = self.computer_socket() else { return };
        let listed = match listed(&socket).await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("could not list the computer service's computers; deletions it owes wait: {e:#}");
                return;
            }
        };
        let (owed, missing): (Vec<String>, Vec<String>) = self.read(|s| {
            // Never one the log has again, should an id ever come back.
            let owed = s.computers.deleted().iter().filter(|id| listed.contains(*id) && s.computers.get(id).is_none()).cloned().collect();
            let missing = s.computers.all().filter(|c| c.status != Status::Queued && !listed.contains(&c.id)).map(|c| c.id.clone()).collect();
            (owed, missing)
        });
        let unrecorded: Vec<String> = {
            let mut u = UNRECORDED.lock().unwrap_or_else(|e| e.into_inner());
            u.retain(|id| listed.contains(id));
            u.iter().cloned().collect()
        };
        for id in owed.iter().chain(&unrecorded) {
            match client::ask(&socket, Request::Destroy { id: id.clone(), delete_volume: true }).await {
                Ok(_) => {
                    tracing::info!("the computer service deleted computer {id}, as the log had");
                    UNRECORDED.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
                }
                Err(e) => tracing::warn!("computer {id} is deleted but the computer service still has it: {e:#}"),
            }
        }
        let mut said = MISSING_SAID.lock().unwrap_or_else(|e| e.into_inner());
        for id in missing {
            if said.insert(id.clone()) {
                tracing::warn!("computer {id} is in the log but the computer service does not have it");
            }
        }
    }
}

/// The computers computerd keeps.
async fn listed(socket: &Path) -> Result<BTreeSet<String>> {
    let v = client::ask(socket, Request::List).await?;
    let ids = v["computers"].as_array().ok_or_else(|| anyhow::anyhow!("the computer service did not list its computers"))?;
    Ok(ids.iter().filter_map(|i| i.as_str().map(str::to_string)).collect())
}

#[cfg(test)]
#[path = "fleet_tests.rs"]
mod tests;
