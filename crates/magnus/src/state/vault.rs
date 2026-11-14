//! The vault's record in the state: the names of the secrets in it, and the
//! kill switch (B11) with what it sealed and revoked. Never a value.

use crate::events::Ev;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

/// A secret the kill switch took out: `at_provider` is None until the
/// provider answered.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Revoked {
    pub adapter: Option<String>,
    pub at_provider: Option<bool>,
    pub detail: Option<String>,
}

#[derive(Default, Debug)]
pub struct Ledger {
    /// The names of the secrets set.
    secrets: HashSet<String>,
    /// Secrets the kill switch emptied from the vault, latest engagement
    /// only: what the owner signs in to or enters again.
    revoked: BTreeMap<String, Revoked>,
    /// Secrets the kill switch kept, sealed: only their provider can revoke them.
    sealed: Vec<String>,
    kill_switch: bool,
    /// Who last engaged or released it, and when (as the log names them).
    kill_switch_by: Option<(String, DateTime<Utc>)>,
}

/// The names of their secrets, which the erasure took from the vault. The
/// kill switch and what it sealed are the server's.
impl crate::forget::Forget for Ledger {
    fn forget(&mut self, who: &crate::forget::Erased) {
        self.secrets.retain(|n| !crate::forget::their_secret(n, &who.person));
        self.revoked.retain(|n, _| !crate::forget::their_secret(n, &who.person));
    }
}

impl Ledger {
    /// `SecretSet`, `VaultSealed`, `SecretRevoked` and `KillSwitch`.
    /// `State::apply` routes no other kind here.
    pub(crate) fn fold(&mut self, ev: &Ev, actor: &str, at: DateTime<Utc>) {
        match ev {
            Ev::SecretSet { name } => {
                self.secrets.insert(name.clone());
                self.revoked.remove(name);
            }
            Ev::KillSwitch { engaged } => {
                self.kill_switch = *engaged;
                self.kill_switch_by = Some((actor.to_string(), at));
            }
            Ev::VaultSealed { revoking, kept } => {
                self.revoked = revoking.iter().map(|n| (n.clone(), Revoked::default())).collect();
                self.sealed = kept.clone();
                for n in revoking {
                    self.secrets.remove(n);
                }
            }
            Ev::SecretRevoked { name, adapter, at_provider, detail } => {
                self.revoked.insert(name.clone(), Revoked { adapter: Some(adapter.clone()), at_provider: Some(*at_provider), detail: Some(detail.clone()) });
            }
            _ => {}
        }
    }

    /// Whether the kill switch is engaged.
    pub fn kill_switch(&self) -> bool {
        self.kill_switch
    }

    /// Who last engaged or released the kill switch, and when.
    pub fn kill_switch_by(&self) -> Option<&(String, DateTime<Utc>)> {
        self.kill_switch_by.as_ref()
    }

    /// What the latest engagement took out of the vault.
    pub fn revoked(&self) -> &BTreeMap<String, Revoked> {
        &self.revoked
    }

    /// What the latest engagement kept, sealed.
    pub fn sealed(&self) -> &[String] {
        &self.sealed
    }

    /// Whether a secret of this name is set.
    pub fn has_secret(&self, name: &str) -> bool {
        self.secrets.contains(name)
    }

    #[cfg(test)]
    pub(crate) fn canonical_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("secrets", format!("{:?}", self.secrets)),
            ("revoked", format!("{:?}", self.revoked)),
            ("sealed", format!("{:?}", self.sealed)),
            ("kill_switch", format!("{:?}", self.kill_switch)),
            ("kill_switch_by", format!("{:?}", self.kill_switch_by)),
        ]
    }
}
