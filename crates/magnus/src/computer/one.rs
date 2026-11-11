//! One computer, always there (spec rev 142). Every person's agent has
//! exactly one computer, and each workspace's team agent has one. Nobody
//! makes one: it is made with the account or the workspace, and on start for
//! anyone who has none yet. Nobody picks one either: the agent's tools always
//! use it, made and woken on first use if need be. Its reset replaces it,
//! never leaving none (`decisions/2026-11-11-one-computer-always-there.md`).
//!
//! An owner with several computers from before keeps the one used last, with
//! its volume; the others go by the usual deletion, which computerd finishes
//! within the purge window, and the owner is told.

use super::{client, service::failed, Change, Computer, KillMode, Ledger, Owner, Role, Status};
use crate::controls::Refused;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::team::Role as WorkspaceRole;
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// How often the server looks for anyone without their computer, after the
/// look at start: an account made some other way gets one within this.
const RECONCILE_EVERY_SECONDS: u64 = 60;
/// computerd's purge window: what a deleted computer held is gone by then.
const PURGE_WINDOW_DAYS: u32 = 30;

/// One owner's computer is made by one caller at a time, so two first uses
/// at once make one computer, not two.
pub(crate) static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

impl Change {
    /// The computer a change is about.
    pub fn computer(&self) -> &str {
        match self {
            Change::Created { id, .. }
            | Change::Granted { id, .. }
            | Change::Queued { id }
            | Change::Started { id }
            | Change::BrowserCreated { id, .. }
            | Change::BrowserStopped { id }
            | Change::SecureFormFilled { id, .. }
            | Change::SurrogateIssued { id, .. }
            | Change::Killed { id, .. }
            | Change::Revoked { id, .. }
            | Change::Hibernated { id }
            | Change::DestinationAdded { id, .. }
            | Change::DestinationGranted { id, .. }
            | Change::PageOpened { id, .. }
            | Change::PageClosed { id, .. }
            | Change::AppInstalled { id, .. }
            | Change::AppRemoved { id, .. }
            | Change::ReplayFailed { id, .. }
            | Change::FileDeleted { id, .. }
            | Change::FreeSpaceZeroed { id, .. }
            | Change::Restored { id, .. }
            | Change::TakeoverStarted { id, .. }
            | Change::TakeoverEnded { id, .. }
            | Change::OperatorGranted { id, .. }
            | Change::OperatorEnded { id, .. }
            | Change::OperatorLooked { id, .. }
            | Change::Kept { id, .. } => id,
        }
    }
}

/// Which of two computers an owner keeps: the one used last, then a running
/// one, then by id, so every replay keeps the same one.
fn kept_first(a: &Computer, b: &Computer) -> Ordering {
    (a.last_used, a.status == Status::Running, &b.id).cmp(&(b.last_used, b.status == Status::Running, &a.id))
}

impl Ledger {
    /// The one computer `owner`'s agent uses. Should it still have several,
    /// the one it keeps.
    pub fn one_of(&self, owner: &Owner) -> Option<&Computer> {
        self.computers.values().filter(|c| c.role == Role::Computer && c.owner == *owner).max_by(|a, b| kept_first(a, b))
    }

    /// Each owner with more than one computer: the one it keeps, then the rest.
    fn crowded(&self) -> Vec<(Computer, Vec<Computer>)> {
        let mut seen: Vec<&Owner> = vec![];
        let mut out = vec![];
        for owner in self.computers.values().filter(|c| c.role == Role::Computer).map(|c| &c.owner) {
            if seen.contains(&owner) {
                continue;
            }
            seen.push(owner);
            let Some(kept) = self.one_of(owner) else { continue };
            let rest: Vec<Computer> = self.computers.values().filter(|c| c.role == Role::Computer && c.owner == *owner && c.id != kept.id).cloned().collect();
            if !rest.is_empty() {
                out.push((kept.clone(), rest));
            }
        }
        out
    }
}

/// What the log says when a computer is deleted: every grant revoked, then
/// the kill that deletes its volume, last because it forgets the computer.
pub(super) fn deletion(c: &Computer) -> Vec<Change> {
    let mut changes: Vec<Change> = c.grants.iter().filter(|g| !g.revoked).map(|g| Change::Revoked { id: c.id.clone(), grant: g.id.clone() }).collect();
    changes.push(Change::Killed { id: c.id.clone(), mode: KillMode::Destroy, delete_volume: true });
    changes
}

/// After the log has forgotten a computer: its secrets go at once, whatever
/// the VM does, then computerd deletes it (and purges it within its window).
pub(super) async fn delete_after(k: &Kernel, socket: &Path, id: &str) -> Result<(), Refused> {
    super::lifecycle::forget_secrets(&k.vault, id).map_err(failed)?;
    super::wake::forget_screen(id);
    client::ask(socket, json!({ "op": "destroy", "id": id, "delete_volume": true })).await.map_err(failed)?;
    Ok(())
}

/// The owner's notice, in words: what was kept and what went.
fn told(owner_words: &str, kept: &Computer, removed: usize) -> String {
    let when = kept.last_used.map(|t| format!(", last used {}", t.format("%-d %B %Y"))).unwrap_or_default();
    let others = if removed == 1 { "1 other computer was".to_string() } else { format!("{removed} other computers were") };
    format!(
        "{owner_words} now one computer, always there. Magnus kept the one used most recently{when}, with its files and apps. {others} removed; everything on them is purged within {PURGE_WINDOW_DAYS} days."
    )
}

impl Kernel {
    /// The computer `owner`'s agent uses, made now if it has none yet.
    /// `None` only on a server without computers.
    pub(crate) async fn computer_for(&self, owner: &Owner) -> Result<Option<String>, Refused> {
        let Ok(socket) = self.computer_socket() else { return Ok(None) };
        let one = |k: &Kernel| k.read(|s| s.computers.one_of(owner).map(|c| c.id.clone()));
        if let Some(id) = one(self) {
            return Ok(Some(id));
        }
        let _one = ONE_AT_A_TIME.lock().await;
        if let Some(id) = one(self) {
            return Ok(Some(id));
        }
        let exists = match owner {
            Owner::Person(p) => self.person(p).is_some(),
            Owner::Workspace(w) => self.read(|s| s.team.workspace(w).is_some()),
        };
        if !exists {
            return Err(Refused::NotFound("person or workspace"));
        }
        let id = super::service::new_id();
        client::ask(&socket, json!({ "op": "create", "id": id })).await.map_err(failed)?;
        self.emit("kernel", Ev::Computer(Change::Created { id: id.clone(), owner: owner.clone() })).await.map_err(failed)?;
        tracing::info!("made computer {id} for {owner:?}");
        Ok(Some(id))
    }

    /// For a new account or workspace: its computer is made in the background.
    pub(crate) fn computer_soon(self: &Arc<Self>, owner: Owner) {
        let k = self.clone();
        tokio::spawn(async move {
            if let Err(e) = k.computer_for(&owner).await {
                tracing::warn!("could not make the computer of {owner:?} yet; the next look tries again: {e:?}");
            }
        });
    }

    /// The caller's computer, or a workspace's they look after. It is never
    /// made by hand: this only finds the one there is (`POST /v1/computers`,
    /// kept for clients from before).
    pub async fn my_computer(&self, who: &crate::config::Person, workspace: Option<&str>) -> Result<Value, Refused> {
        let owner = match workspace {
            Some(w) => {
                let admin = who.is_owner() || who.has_role("admin") || self.read(|s| s.team.role(w, &who.id).is_some_and(|r| r >= WorkspaceRole::Admin));
                if !admin {
                    return Err(Refused::Denied("only an admin looks after a workspace's computer".into()));
                }
                Owner::Workspace(w.to_string())
            }
            None => Owner::Person(who.id.clone()),
        };
        let id = self.computer_for(&owner).await?.ok_or_else(|| Refused::Conflict("this server has no computers".into()))?;
        Ok(json!({ "id": id }))
    }

    /// Deletes `c` by the usual deletion, for a workspace that is gone.
    pub(crate) async fn delete_computer(&self, actor: &str, c: &Computer) -> Result<(), Refused> {
        let socket = self.computer_socket()?;
        let evs: Vec<(&str, Ev)> = deletion(c).into_iter().map(|ch| (actor, Ev::Computer(ch))).collect();
        self.emit_all(evs).await.map_err(failed)?;
        delete_after(self, &socket, &c.id).await
    }

    /// At start and every minute after: an owner with several computers
    /// keeps one, a workspace that is gone loses its computer, and every
    /// person and workspace without one gets it. Safe to repeat.
    pub(crate) async fn reconcile_computers(&self) {
        let Ok(socket) = self.computer_socket() else { return };
        let crowded = self.read(|s| s.computers.crowded());
        for (kept, rest) in crowded {
            if let Err(e) = self.keep_one(&socket, &kept, &rest).await {
                tracing::warn!("could not keep one computer for {:?}: {e:?}", kept.owner);
            }
        }
        let orphans: Vec<Computer> = self.read(|s| s.computers.all().filter(|c| matches!(&c.owner, Owner::Workspace(w) if s.team.workspace(w).is_none())).cloned().collect());
        for c in orphans {
            if let Err(e) = self.delete_computer("kernel", &c).await {
                tracing::warn!("could not delete computer {} of a deleted workspace: {e:?}", c.id);
            }
        }
        let mut owners: Vec<Owner> = self.everyone().into_iter().map(|(id, _)| Owner::Person(id)).collect();
        owners.extend(self.read(|s| s.team.all().map(|w| Owner::Workspace(w.id.clone())).collect::<Vec<_>>()));
        for owner in owners {
            if let Err(e) = self.computer_for(&owner).await {
                tracing::warn!("could not make the computer of {owner:?} yet: {e:?}");
            }
        }
    }

    /// One write: the others deleted, the one kept, and its people told.
    async fn keep_one(&self, socket: &Path, kept: &Computer, rest: &[Computer]) -> Result<(), Refused> {
        let mut evs: Vec<(&str, Ev)> = rest.iter().flat_map(deletion).map(|ch| ("kernel", Ev::Computer(ch))).collect();
        let removed: Vec<String> = rest.iter().map(|c| c.id.clone()).collect();
        evs.push(("kernel", Ev::Computer(Change::Kept { id: kept.id.clone(), owner: kept.owner.clone(), removed: removed.clone() })));
        let (people, words) = self.read(|s| match &kept.owner {
            Owner::Person(p) => (vec![p.clone()], "You have".to_string()),
            Owner::Workspace(w) => {
                let ws = s.team.workspace(w);
                let admins = ws.map(|ws| ws.members.iter().filter(|(_, r)| **r >= WorkspaceRole::Admin).map(|(p, _)| p.clone()).collect()).unwrap_or_default();
                (admins, format!("The {} team agent has", ws.map_or("workspace's", |ws| ws.name.as_str())))
            }
        });
        let summary = told(&words, kept, rest.len());
        for person in people {
            let details = json!({ "reason": "one_computer", "kept": kept.id, "kept_last_used": kept.last_used, "removed": removed, "purge_days": PURGE_WINDOW_DAYS });
            evs.push(("kernel", self.notice_to(None, None, person, summary.clone(), details)));
        }
        self.emit_all(evs).await.map_err(failed)?;
        tracing::info!("{:?} keeps computer {} and no longer has {removed:?}", kept.owner, kept.id);
        for id in &removed {
            if let Err(e) = delete_after(self, socket, id).await {
                tracing::warn!("computer {id} is forgotten but computerd did not delete it yet: {e:?}");
            }
        }
        Ok(())
    }
}

pub(super) fn schedule(k: Arc<Kernel>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(RECONCILE_EVERY_SECONDS));
        loop {
            tick.tick().await;
            k.reconcile_computers().await;
        }
    });
}

#[cfg(test)]
#[path = "one_tests.rs"]
mod tests;
