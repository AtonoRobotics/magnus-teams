//! The one store for app sign-ins: each person's or shared project's own
//! sign-in to an app, and a person's Link wallet. Every read and write of
//! one goes through here: its fresh token, a new sign-in replacing it, more
//! access asked of it, and its end.
//!
//! Each sign-in has its own turn for refreshing, so one slow token endpoint
//! holds up only the calls through that one sign-in. Each has a generation,
//! moved on by every replacement and every end: a refresh writes back only
//! onto the sign-in it read, so a Disconnect or a new sign-in made while a
//! refresh was on its way is never undone by it. While a sign-in is ending
//! (revoked at the app, not yet deleted here), no one is given it.

use crate::vault::{Secret, Vault};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// What a workspace's key starts with where a person's id stands for a
/// person's (`team::sign_in::owner_key`). A person id has no `:`
/// (`accounts::valid_person_id`), so neither is ever read as the other.
const PROJECT_PREFIX: &str = "workspace:";

/// Whose a sign-in to an app is: a person's own, or a shared project's.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SignInOwner {
    Person(String),
    Project(String),
}

impl SignInOwner {
    /// The owner a sign-in is kept under: a person's id, or a project's key.
    pub(crate) fn of_key(key: &str) -> SignInOwner {
        match key.strip_prefix(PROJECT_PREFIX) {
            Some(ws) => SignInOwner::Project(ws.to_string()),
            None => SignInOwner::Person(key.to_string()),
        }
    }

    /// The key it is kept under in the vault's names.
    pub(crate) fn key(&self) -> String {
        match self {
            SignInOwner::Person(p) => p.clone(),
            SignInOwner::Project(ws) => format!("{PROJECT_PREFIX}{ws}"),
        }
    }

    /// Whose sign-in to `app` a vault name is; None when it is not one (the
    /// old shared sign-in, another app's, or anything else).
    pub(crate) fn of_name(name: &str, app: &str) -> Option<SignInOwner> {
        let key = name.strip_prefix(&format!("{}__", super::credential_name(app)))?;
        (!key.is_empty()).then(|| SignInOwner::of_key(key))
    }

    /// The app and owner of any app sign-in's vault name. App names never
    /// hold `__`, so the owner is what follows the first one.
    pub(crate) fn parse(name: &str) -> Option<(String, SignInOwner)> {
        let (app, key) = name.strip_prefix("connector_")?.split_once("__")?;
        (!app.is_empty() && !key.is_empty()).then(|| (app.to_string(), SignInOwner::of_key(key)))
    }
}

#[derive(Default)]
struct Slot {
    generation: u64,
    ending: bool,
    refreshing: Arc<tokio::sync::Mutex<()>>,
}

/// Every sign-in's turn and generation, by its vault name.
#[derive(Default)]
pub(crate) struct SignInStore {
    slots: Mutex<HashMap<String, Slot>>,
}

/// Why a refresh's write-back was refused.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Superseded {
    /// Replaced by a new sign-in since: what the refresh made is left alone.
    Replaced,
    /// Ended, or ending: what the refresh made is to be revoked too.
    Ended,
}

impl SignInStore {
    fn slots(&self) -> MutexGuard<'_, HashMap<String, Slot>> {
        self.slots.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// This sign-in's own turn to refresh. One refresh of a sign-in at a
    /// time (a refresh token may work only once); no other sign-in waits.
    pub(crate) async fn refreshing(&self, name: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let turn = self.slots().entry(name.to_string()).or_default().refreshing.clone();
        turn.lock_owned().await
    }

    /// The sign-in as stored now, and its generation. Nothing while it is
    /// ending.
    pub(crate) fn read(&self, vault: &Vault, name: &str) -> Result<(u64, Option<Secret>)> {
        let slots = self.slots();
        match slots.get(name) {
            Some(s) if s.ending => Ok((s.generation, None)),
            s => Ok((s.map_or(0, |s| s.generation), vault.get(name)?)),
        }
    }

    pub(crate) fn get(&self, vault: &Vault, name: &str) -> Result<Option<Secret>> {
        Ok(self.read(vault, name)?.1)
    }

    /// A new sign-in in place of whatever was there.
    pub(crate) fn replace(&self, vault: &Vault, name: &str, value: &str) -> Result<()> {
        let mut slots = self.slots();
        let s = slots.entry(name.to_string()).or_default();
        s.generation += 1;
        s.ending = false;
        vault.put(name, value)
    }

    /// Write back what a refresh made of the sign-in it read at
    /// `generation`: `f` is given the sign-in as stored now and gives what
    /// to keep. Refused once the sign-in was replaced or ended since.
    pub(crate) fn write_back(&self, vault: &Vault, name: &str, generation: u64, f: impl FnOnce(&str) -> Result<String>) -> Result<Result<(), Superseded>> {
        let slots = self.slots();
        let s = slots.get(name);
        if s.is_some_and(|s| s.ending) {
            return Ok(Err(Superseded::Ended));
        }
        let Some(now) = vault.get(name)? else { return Ok(Err(Superseded::Ended)) };
        if s.map_or(0, |s| s.generation) != generation {
            return Ok(Err(Superseded::Replaced));
        }
        vault.put(name, &f(now.expose())?)?;
        Ok(Ok(()))
    }

    /// Change the sign-in in place (more access asked of it): `f` is given
    /// it as stored now. Nothing when there is none.
    pub(crate) fn amend(&self, vault: &Vault, name: &str, f: impl FnOnce(&str) -> Result<String>) -> Result<()> {
        let slots = self.slots();
        if slots.get(name).is_some_and(|s| s.ending) {
            return Ok(());
        }
        let Some(now) = vault.get(name)? else { return Ok(()) };
        vault.put(name, &f(now.expose())?)
    }

    /// Begin ending a sign-in: from now on no one is given it and no
    /// refresh writes it back. What was stored, and the generation to
    /// finish (`ended`) or give it back (`kept`) with. None: there was none.
    pub(crate) fn ending(&self, vault: &Vault, name: &str) -> Result<Option<(u64, Secret)>> {
        let mut slots = self.slots();
        let s = slots.entry(name.to_string()).or_default();
        if s.ending {
            return Ok(None);
        }
        let Some(stored) = vault.get(name)? else { return Ok(None) };
        s.generation += 1;
        s.ending = true;
        Ok(Some((s.generation, stored)))
    }

    /// Finish ending it: deleted here, unless a new sign-in took its place
    /// meanwhile.
    pub(crate) fn ended(&self, vault: &Vault, name: &str, generation: u64) -> Result<()> {
        let mut slots = self.slots();
        let s = slots.entry(name.to_string()).or_default();
        if s.generation == generation && s.ending {
            s.ending = false;
            vault.remove(name)?;
        }
        Ok(())
    }

    /// It was not ended after all (the app refused to end it): it is given
    /// out again, unless a new sign-in took its place meanwhile.
    pub(crate) fn kept(&self, name: &str, generation: u64) {
        if let Some(s) = self.slots().get_mut(name).filter(|s| s.generation == generation) {
            s.ending = false;
        }
    }

    /// End it at once, here: what was stored.
    pub(crate) fn end(&self, vault: &Vault, name: &str) -> Result<Option<Secret>> {
        match self.ending(vault, name)? {
            Some((generation, stored)) => {
                self.ended(vault, name, generation)?;
                Ok(Some(stored))
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
#[path = "sign_in_store_tests.rs"]
mod tests;
