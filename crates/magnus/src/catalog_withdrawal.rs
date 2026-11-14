//! Withdrawing a catalog connector between releases (connectors spec,
//! Lifecycle: "Withdrawal: appliances disable the connector, revoke its
//! connections and tell affected users in their Library";
//! decisions/2026-11-13-catalog-withdrawal.md).
//!
//! The manifests themselves stay compiled in, covered by the release's
//! signature. What can change between releases is only the withdrawal list:
//! `catalog/withdrawn.json` beside the server's update feed, signed with the
//! same release key and mechanism as a release package (`tauri signer sign
//! --app-version`, verified by `update::verify`), the version it binds being
//! `catalog-withdrawals.<serial>`. The server reads it when it starts and on
//! its update-check cadence. A list that is unsigned, does not verify, or is
//! older than the one applied changes nothing: the last good one stands, and
//! a missing list withdraws and restores nothing.
//!
//! For each withdrawn connector whose compiled manifest's version is in the
//! named range: it is offered to no one and reached by no run (left out of
//! `Kernel::all_connectors`), each person's connection is revoked at the
//! provider and deleted (`Kernel::disconnect`), and each person who had one
//! gets a notice. A withdrawal is one-way until a later signed list leaves
//! the connector out; a newer manifest outside the range ships it again.

use crate::catalog::Manifest;
use crate::events::Ev;
use crate::kernel::Kernel;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// The list's directory under the updates directory, and its file; the
/// signature is the file's name with `.sig`, as `tauri signer` writes it.
pub const DIR: &str = "catalog";
pub const FILE: &str = "withdrawn.json";
/// Who the log names for what a withdrawal does.
pub const ACTOR: &str = "kernel:catalog";
/// The largest list read: a few hundred entries with their reasons.
const MAX_LIST_BYTES: u64 = 1024 * 1024;
const SECONDS_PER_MINUTE: u64 = 60;

/// The version a list's signature binds: its serial, named so that neither
/// a release package's signature nor another list's passes for it.
pub fn bound_version(serial: u64) -> String {
    format!("catalog-withdrawals.{serial}")
}

/// One withdrawn connector: its catalog id, the manifest versions withdrawn
/// (a semver requirement such as `<1.4.0`; none, or one that does not
/// parse, is every version) and the reason people are told.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Withdrawal {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub versions: Option<String>,
    #[serde(default)]
    pub reason: String,
}

impl Withdrawal {
    /// Whether a manifest at `version` is withdrawn. A range that does not
    /// parse withdraws every version: the safer reading of a signed list.
    pub fn covers(&self, version: &str) -> bool {
        let Some(req) = &self.versions else { return true };
        match (semver::VersionReq::parse(req), semver::Version::parse(version)) {
            (Ok(r), Ok(v)) => r.matches(&v),
            _ => true,
        }
    }
}

/// The published list. `serial` rises with every change to it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct List {
    pub serial: u64,
    #[serde(default)]
    pub withdrawn: Vec<Withdrawal>,
}

/// What withdrawal did. A kind is never edited or deleted.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    /// The list with this serial named the connector.
    Withdrawn { serial: u64, withdrawal: Withdrawal },
    /// A later list no longer names it.
    Restored { serial: u64, id: String },
    /// The list with this serial is the one applied: an older one is refused.
    Applied { serial: u64 },
    /// A person who had a connection to it was told, with this inbox item.
    Told { id: String, person: String, item: String },
}

/// The withdrawals in force, from the log.
#[derive(Clone, Debug, Default)]
pub struct Ledger {
    pub serial: u64,
    pub withdrawn: BTreeMap<String, Withdrawal>,
    told: BTreeSet<(String, String)>,
}

impl Ledger {
    pub fn apply(&mut self, c: &Change) {
        match c {
            Change::Withdrawn { withdrawal, .. } => {
                self.withdrawn.insert(withdrawal.id.clone(), withdrawal.clone());
            }
            Change::Restored { id, .. } => {
                self.withdrawn.remove(id);
                // Withdrawn again later, the people connected then are told again.
                self.told.retain(|(i, _)| i != id);
            }
            Change::Applied { serial } => self.serial = self.serial.max(*serial),
            Change::Told { id, person, .. } => {
                self.told.insert((id.clone(), person.clone()));
            }
        }
    }

    /// Whether this release's manifest is withdrawn.
    pub fn covers(&self, m: &Manifest) -> bool {
        self.withdrawn.get(&m.id).is_some_and(|w| w.covers(&m.version))
    }

    pub fn told(&self, id: &str, person: &str) -> bool {
        self.told.contains(&(id.to_string(), person.to_string()))
    }
}

/// Where the list is published for this server.
pub fn list_dir(cfg: &crate::config::Config) -> PathBuf {
    cfg.updates_dir().join(DIR)
}

/// The list published in `dir`, verified with the release key. None: none
/// is published. An error: one is, and it is not to be trusted.
pub fn read(dir: &Path, public_key: &str) -> Result<Option<List>> {
    let path = dir.join(FILE);
    let Ok(meta) = std::fs::metadata(&path) else { return Ok(None) };
    anyhow::ensure!(meta.len() <= MAX_LIST_BYTES, "the withdrawal list is larger than {MAX_LIST_BYTES} bytes");
    let bytes = std::fs::read(&path).context("read the withdrawal list")?;
    let sig = std::fs::read_to_string(dir.join(format!("{FILE}.sig"))).context("the withdrawal list is not signed")?;
    // Read for its serial only; nothing in it is used until it verifies.
    let list: List = serde_json::from_slice(&bytes).context("the withdrawal list does not parse")?;
    crate::update::verify(&bytes, &sig, public_key, &bound_version(list.serial)).context("the withdrawal list")?;
    anyhow::ensure!(list.withdrawn.iter().all(|w| !w.id.is_empty()), "the withdrawal list names a connector without its id");
    Ok(Some(list))
}

/// The changes that take the ledger to `list`: none when the list is the
/// one applied; refused when it is older.
pub fn changes(had: &Ledger, list: &List) -> Result<Vec<Change>> {
    anyhow::ensure!(list.serial >= had.serial, "the withdrawal list's serial {} is older than the applied {}", list.serial, had.serial);
    if list.serial == had.serial {
        return Ok(vec![]);
    }
    let mut out = vec![];
    let named: BTreeMap<&str, &Withdrawal> = list.withdrawn.iter().map(|w| (w.id.as_str(), w)).collect();
    for (id, w) in &named {
        if had.withdrawn.get(*id) != Some(*w) {
            out.push(Change::Withdrawn { serial: list.serial, withdrawal: (*w).clone() });
        }
    }
    for id in had.withdrawn.keys().filter(|id| !named.contains_key(id.as_str())) {
        out.push(Change::Restored { serial: list.serial, id: id.clone() });
    }
    out.push(Change::Applied { serial: list.serial });
    Ok(out)
}

impl Kernel {
    /// Read the published withdrawal list, apply it when it is newer than
    /// the one applied, and end every connection to a withdrawn connector.
    /// What was done, for the log and the tests.
    pub async fn check_withdrawals(self: &Arc<Self>) -> Result<Value> {
        let read = read(&list_dir(&self.cfg), &self.cfg.updates.public_key);
        let refused = match read {
            Ok(Some(list)) => match changes(&self.read(|s| s.catalog.clone()), &list) {
                Ok(cs) => {
                    for c in cs {
                        self.emit(ACTOR, Ev::Catalog(c)).await?;
                    }
                    None
                }
                Err(e) => Some(format!("{e:#}")),
            },
            Ok(None) => None,
            Err(e) => Some(format!("{e:#}")),
        };
        if let Some(r) = &refused {
            tracing::error!("catalog withdrawals: {r}; the last good list stands");
        }
        let ended = self.end_withdrawn_connections().await?;
        Ok(json!({ "serial": self.read(|s| s.catalog.serial), "refused": refused, "connections_ended": ended }))
    }

    /// Every connection to a withdrawn catalog connector: its person told
    /// once, then the connection revoked at the provider and deleted. Run
    /// again after a crash, it finishes what was left.
    async fn end_withdrawn_connections(&self) -> Result<usize> {
        let ledger = self.read(|s| s.catalog.clone());
        let mut ended = 0;
        for m in crate::catalog::manifests().iter().filter(|m| ledger.covers(m)) {
            let Some(w) = ledger.withdrawn.get(&m.id) else { continue };
            let shared = crate::connectors::credential_name(&m.id);
            let prefix = format!("{shared}__");
            for name in self.vault.names()?.into_iter().filter(|n| crate::connectors::is_sign_in_of(n, &m.id)) {
                let Some(person) = name.strip_prefix(&prefix).map(str::to_string) else {
                    // The old shared sign-in, no one's to be told of.
                    if let Some(s) = self.vault.remove(&name)? {
                        let (done, detail) = crate::connectors::revoke(s.expose()).await;
                        tracing::info!("{}: withdrawn; the shared sign-in revoked {done}: {detail}", m.id);
                    }
                    continue;
                };
                if !self.read(|s| s.catalog.told(&m.id, &person)) {
                    self.tell_withdrawn(m, w, &person).await?;
                }
                self.disconnect(&m.id, &person, ACTOR).await?;
                ended += 1;
            }
        }
        Ok(ended)
    }

    async fn tell_withdrawn(&self, m: &Manifest, w: &Withdrawal, person: &str) -> Result<()> {
        let why = if w.reason.trim().is_empty() { String::new() } else { format!(" {}", w.reason.trim()) };
        let summary = format!("{} was taken out of Magnus.{why} Your connection to it has been ended, and Magnus will not use it.", m.name);
        let details = json!({ "connector": m.id, "name": m.name, "withdrawn": true, "reason": w.reason, "remove_access": m.remove_access });
        let (item, ev) = crate::kernel::notice_event(None, None, person.to_string(), summary, details);
        self.emit_all(vec![(ACTOR, ev), (ACTOR, Ev::Catalog(Change::Told { id: m.id.clone(), person: person.to_string(), item }))]).await?;
        Ok(())
    }
}

/// Look once at start, then on the update check's cadence. Withdrawal is
/// for safety, so it is read even where updates are installed by hand.
pub fn schedule(k: Arc<Kernel>) {
    let minutes = k.cfg.updates.check_minutes;
    tokio::spawn(async move {
        if let Err(e) = k.check_withdrawals().await {
            tracing::error!("catalog withdrawals: {e:#}");
        }
        if minutes == 0 {
            return;
        }
        let mut t = tokio::time::interval(Duration::from_secs(u64::from(minutes) * SECONDS_PER_MINUTE));
        t.tick().await;
        loop {
            t.tick().await;
            if let Err(e) = k.check_withdrawals().await {
                tracing::error!("catalog withdrawals: {e:#}");
            }
        }
    });
}

#[cfg(test)]
#[path = "catalog_withdrawal_tests.rs"]
mod tests;
