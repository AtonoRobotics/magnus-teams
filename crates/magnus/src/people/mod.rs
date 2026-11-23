//! Everyone the server keeps, read one way (decision
//! `2026-11-14-one-people-directory.md`).
//!
//! A person comes from the config's `[[people]]` or from the server's own
//! directory (`accounts`), and what the log said since lies over them: roles
//! set here (`RolesSet`), then the identity provider's groups and roles. The
//! config wins a namesake. Every list of people, every "is this a person",
//! every name and every "is this an admin" is read here, so none of them can
//! forget the server's own people or one of the two places "admin" is kept.

mod authority;
#[cfg(test)]
mod guard_tests;
#[cfg(test)]
mod kernel_tests;
mod rights;
#[cfg(test)]
mod rights_tests;
#[cfg(test)]
mod tests;

pub use authority::Authority;
pub use rights::{may_use_password, rights, Rights};

use crate::accounts::{Accounts, LocalPerson};
use crate::config::Person;
use crate::kernel::Kernel;
use std::collections::BTreeSet;

/// The server's own directory as the merge reads it. Postgres keeps it on a
/// server (`Accounts`, from its cache); a table stands in for it in tests.
pub trait Local {
    /// Every id it keeps.
    fn local_ids(&self) -> Vec<String>;
    /// Its record for `id`.
    fn local(&self, id: &str) -> Option<LocalPerson>;
}

impl Local for Accounts {
    fn local_ids(&self) -> Vec<String> {
        self.local_people().into_iter().map(|(id, _)| id).collect()
    }

    fn local(&self, id: &str) -> Option<LocalPerson> {
        self.local_person(id)
    }
}

pub type ProviderOverlay = (Option<Vec<String>>, Option<Vec<String>>);

/// What the log has said about one person's rights since the config:
/// roles set here, and the identity provider's groups and roles.
#[derive(Clone, Debug, Default)]
pub struct Overlay {
    pub roles: Option<Vec<String>>,
    pub provider: Option<ProviderOverlay>,
}

/// The record a sign-in resolves: the config first, else the server's own
/// directory, where a person has no token, groups or roles: a member,
/// nothing more.
pub fn resolve(config: &[Person], local: &dyn Local, id: &str) -> Option<Person> {
    if let Some(p) = config.iter().find(|p| p.id == id) {
        return Some(p.clone());
    }
    let l = local.local(id)?;
    Some(Person { id: id.to_string(), name: l.name, token: None, groups: vec![], roles: vec![] })
}

/// `p` with the log's overlay on it: roles set here replace the record's,
/// and the identity provider's groups and roles, when it names them,
/// replace both.
pub fn overlaid(mut p: Person, o: Overlay) -> Person {
    if let Some(r) = o.roles {
        p.roles = r;
    }
    if let Some((groups, roles)) = o.provider {
        if let Some(g) = groups {
            p.groups = g;
        }
        if let Some(r) = roles {
            p.roles = r;
        }
    }
    p
}

/// Every id, once each, in order.
pub fn ids(config: &[Person], local: &dyn Local) -> Vec<String> {
    let mut all: BTreeSet<String> = config.iter().map(|p| p.id.clone()).collect();
    all.extend(local.local_ids());
    all.into_iter().collect()
}

/// Every id with the config's first, in its order, then the server's own:
/// where "the first owner" is read, it is the config's.
fn config_first(config: &[Person], local: &dyn Local) -> Vec<String> {
    let mut out: Vec<String> = config.iter().map(|p| p.id.clone()).collect();
    let mut rest: Vec<String> = local.local_ids().into_iter().filter(|id| !out.contains(id)).collect();
    rest.sort();
    out.append(&mut rest);
    out
}

/// Whether `id` is a person here.
pub fn exists(config: &[Person], local: &dyn Local, id: &str) -> bool {
    config.iter().any(|p| p.id == id) || local.local(id).is_some()
}

/// A person's name to show: their name, or their id when they have none.
pub fn shown_name(p: Option<Person>, id: &str) -> String {
    p.map(|p| p.name).filter(|n| !n.is_empty()).unwrap_or_else(|| id.to_string())
}

/// The kernel's directory: everyone the server keeps, with the log's
/// overlays and the testers group on them.
pub struct People<'k> {
    k: &'k Kernel,
}

impl Kernel {
    pub fn people(&self) -> People<'_> {
        People { k: self }
    }

    /// One person by id (`People::get`).
    pub fn person(&self, id: &str) -> Option<Person> {
        self.people().get(id)
    }

    /// What `p` may administer on the server: their roles here and the
    /// server's own directory, read fresh. See [`Authority`].
    pub async fn authority(&self, p: &Person) -> Authority {
        Authority::of(p, self.accounts.role(&p.id).await)
    }
}

impl People<'_> {
    /// One person by id, as every check reads them.
    pub fn get(&self, id: &str) -> Option<Person> {
        let k = self.k;
        let p = resolve(&k.cfg.people, &k.accounts, id)?;
        let o = k.read(|s| Overlay { roles: s.identity.roles_set(id).cloned(), provider: s.identity.provider_rights(id).cloned() });
        let mut p = overlaid(p, o);
        crate::legal::testers_group(k, &mut p);
        Some(p)
    }

    /// Whether `id` is a person here, in the config or the server's own
    /// directory.
    pub fn exists(&self, id: &str) -> bool {
        exists(&self.k.cfg.people, &self.k.accounts, id)
    }

    /// Every person's id, once each, in order.
    pub fn ids(&self) -> Vec<String> {
        ids(&self.k.cfg.people, &self.k.accounts)
    }

    /// Every person, in id order.
    pub fn all(&self) -> Vec<Person> {
        self.ids().iter().filter_map(|id| self.get(id)).collect()
    }

    /// Everyone by id and name, ordered by name.
    pub fn named(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = self.ids().into_iter().map(|id| (id.clone(), self.name(&id))).collect();
        v.sort_by_key(|p| p.1.to_lowercase());
        v
    }

    /// A person's name to show, or the id when they have none or are gone.
    pub fn name(&self, id: &str) -> String {
        shown_name(self.get(id), id)
    }

    /// The owners: the config's first, in its order, then the server's own.
    pub fn owners(&self) -> Vec<Person> {
        config_first(&self.k.cfg.people, &self.k.accounts).iter().filter_map(|id| self.get(id)).filter(|p| rights(p).is_server_owner()).collect()
    }

    /// Everyone who administers the server ([`Authority::administers`]).
    pub async fn admins(&self) -> Vec<String> {
        let mut out = vec![];
        for p in self.all() {
            if self.k.authority(&p).await.administers() {
                out.push(p.id);
            }
        }
        out
    }
}
