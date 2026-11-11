//! Resetting a computer, and what the vault forgets when one is deleted.
//!
//! The vault holds two things for one computer: the certificate authority its
//! proxy signs with, and the real credentials behind its surrogates. Deleting
//! the computer deletes both. A computer the log no longer knows (deleted
//! before this existed, or by a kill cut short) is swept at start.

use super::{Change, Computer, KillMode, Owner};
use crate::config::Person;
use crate::controls::Refused;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::vault::Vault;
use anyhow::Result;
use serde_json::{json, Value};

/// The vault names that belong to one computer and to nothing else.
fn belongs_to(name: &str, computer: &str) -> bool {
    name == super::egress::ca_secret_name(computer) || name.strip_prefix("surrogate/").and_then(|rest| rest.strip_prefix(computer)).is_some_and(|rest| rest.starts_with('/'))
}

/// The computer a computer-scoped vault name belongs to, if it is one.
fn owner_of(name: &str) -> Option<&str> {
    if let Some(id) = name.strip_prefix(super::egress::CA_SECRET_PREFIX) {
        return Some(id);
    }
    name.strip_prefix("surrogate/").and_then(|rest| rest.split_once('/')).map(|(id, _)| id)
}

/// Removes every vault entry of `computer`. Returns how many went.
pub(crate) fn forget_secrets(vault: &Vault, computer: &str) -> Result<usize> {
    let mine: Vec<String> = vault.names()?.into_iter().filter(|n| belongs_to(n, computer)).collect();
    for n in &mine {
        vault.remove(n)?;
    }
    Ok(mine.len())
}

/// Removes the real credentials behind `computer`'s surrogates, and keeps its
/// certificate authority: an erased computer is the same computer.
pub(crate) fn forget_surrogates(vault: &Vault, computer: &str) -> Result<usize> {
    let mine: Vec<String> = vault.names()?.into_iter().filter(|n| belongs_to(n, computer) && *n != super::egress::ca_secret_name(computer)).collect();
    for n in &mine {
        vault.remove(n)?;
    }
    Ok(mine.len())
}

/// Removes the computer-scoped entries of every computer `known` says is gone.
pub(crate) fn sweep_secrets(vault: &Vault, known: impl Fn(&str) -> bool) -> Result<usize> {
    let orphans: Vec<String> = vault.names()?.into_iter().filter(|n| owner_of(n).is_some_and(|id| !known(id))).collect();
    for n in &orphans {
        vault.remove(n)?;
    }
    Ok(orphans.len())
}

impl Kernel {
    /// Back to the base image (spec rev 141, 142): the same computer, its VM,
    /// overlay and snapshot gone and every grant revoked, rebuilt on its next
    /// use from the base image and its manifest, its apps installed again.
    /// With `keep_files` the person's volume stays; without it the volume is
    /// erased and an empty one takes its place.
    pub async fn reset_computer(&self, who: &Person, id: &str, keep_files: bool) -> Result<Value, Refused> {
        let c = self.controlled_computer(who, id)?;
        if keep_files {
            let killed = self.kill_computer(who, id, KillMode::Destroy, false).await?;
            return Ok(json!({ "id": id, "revoked": killed["revoked"] }));
        }
        self.erase_computer(who, &c).await
    }

    /// A reset that erases the files. The computer stays, with its id, owner
    /// and manifest: only its volume is replaced by an empty one, the old one
    /// and its backups deleted as a deleted computer's are, with the
    /// passwords and tokens the vault kept for it. Its owner is never without
    /// it. The log first, so nothing acts on the old files from here on.
    pub(super) async fn erase_computer(&self, who: &Person, c: &Computer) -> Result<Value, Refused> {
        if matches!(&c.owner, Owner::Person(p) if *p != who.id) {
            return Err(Refused::Denied("a volume is deleted only when the person it belongs to asks".into()));
        }
        let socket = self.computer_socket()?;
        let id = c.id.as_str();
        let mut changes: Vec<Change> = c.grants.iter().filter(|g| !g.revoked).map(|g| Change::Revoked { id: id.into(), grant: g.id.clone() }).collect();
        let revoked = changes.len();
        changes.push(Change::Killed { id: id.into(), mode: KillMode::Destroy, delete_volume: false });
        changes.push(Change::VolumeErased { id: id.into() });
        let actor = format!("person:{}", who.id);
        self.emit_all(changes.into_iter().map(|ch| (actor.as_str(), Ev::Computer(ch))).collect()).await.map_err(super::service::failed)?;
        forget_surrogates(&self.vault, id).map_err(super::service::failed)?;
        super::wake::forget_screen(id);
        super::client::ask(&socket, json!({ "op": "erase_volume", "id": id })).await.map_err(super::service::failed)?;
        Ok(json!({ "id": id, "revoked": revoked, "erased": true }))
    }

    /// At start: the vault keeps nothing for a computer the log has forgotten.
    pub(super) fn sweep_computer_secrets(&self) {
        match sweep_secrets(&self.vault, |id| self.read(|s| s.computers.get(id).is_some())) {
            Ok(0) => {}
            Ok(n) => tracing::info!("removed {n} vault entries of deleted computers"),
            Err(e) => tracing::warn!("could not sweep deleted computers' vault entries: {e:#}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> (tempfile::TempDir, Vault) {
        let d = tempfile::tempdir().unwrap();
        let v = Vault::open(d.path()).unwrap();
        (d, v)
    }

    fn fill(v: &Vault) {
        for n in ["computer-ca-aa", "surrogate/aa/github", "surrogate/aa/stripe", "computer-ca-aab", "surrogate/aab/github", "connector/aa", "identity/aa/password"] {
            v.put(n, "secret").unwrap();
        }
    }

    #[test]
    fn deleting_a_computer_takes_its_vault_entries_and_no_one_elses() {
        let (_d, v) = vault();
        fill(&v);
        assert_eq!(forget_secrets(&v, "aa").unwrap(), 3);
        let left = v.names().unwrap();
        assert_eq!(left, ["computer-ca-aab", "connector/aa", "identity/aa/password", "surrogate/aab/github"]);
    }

    #[test]
    fn the_sweep_removes_only_the_entries_of_computers_the_log_forgot() {
        let (_d, v) = vault();
        fill(&v);
        assert_eq!(sweep_secrets(&v, |id| id == "aab").unwrap(), 3);
        let left = v.names().unwrap();
        assert_eq!(left, ["computer-ca-aab", "connector/aa", "identity/aa/password", "surrogate/aab/github"]);
    }

    #[test]
    fn a_sealed_vault_still_forgets() {
        let (_d, v) = vault();
        fill(&v);
        v.seal();
        assert_eq!(forget_secrets(&v, "aa").unwrap(), 3);
        assert!(!v.names().unwrap().iter().any(|n| n == "computer-ca-aa"));
    }
}
