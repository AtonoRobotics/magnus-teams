//! Which models each role or person may use. With no rule for a person or
//! any of their roles, they may use every model the server offers. A
//! person's own rule wins over their roles'; several roles' rules add up,
//! and everyone has the role `member`, so its rule is what everyone may use.
//! The owner pays and sets the rules, so the owner is never limited.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// The role every person has, so a rule for it covers everyone without
/// one of their own or a more specific role's.
pub const EVERYONE: &str = "member";

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Access {
    pub roles: BTreeMap<String, BTreeSet<String>>,
    pub people: BTreeMap<String, BTreeSet<String>>,
}

impl Access {
    pub fn set(&mut self, role: Option<&str>, person: Option<&str>, models: Option<Vec<String>>) {
        let map = match (role, person) {
            (_, Some(p)) => self.people.entry(p.to_string()),
            (Some(r), None) => self.roles.entry(r.to_string()),
            (None, None) => return,
        };
        match (map, models) {
            (std::collections::btree_map::Entry::Occupied(e), None) => {
                e.remove();
            }
            (std::collections::btree_map::Entry::Vacant(_), None) => {}
            (e, Some(m)) => {
                *e.or_default() = m.into_iter().collect();
            }
        }
    }

    /// The models (`adapter/model`) the person may use, or None when they
    /// may use all. `roles` are theirs, `member` among them.
    pub fn allowed(&self, person: &str, roles: &[String]) -> Option<BTreeSet<String>> {
        if roles.iter().any(|r| r == "owner") {
            return None;
        }
        if let Some(m) = self.people.get(person) {
            return Some(m.clone());
        }
        // Each of their roles that has a rule adds its models; a role
        // without one adds nothing, so a role nobody wrote a rule for never
        // undoes the rule for everyone.
        let ruled: Vec<&BTreeSet<String>> = roles.iter().chain(std::iter::once(&EVERYONE.to_string())).filter_map(|r| self.roles.get(r)).collect();
        if ruled.is_empty() {
            return None;
        }
        Some(ruled.into_iter().flatten().cloned().collect())
    }
}

/// Whether `adapter/model` is among the allowed, None meaning all.
pub fn permits(allowed: &Option<BTreeSet<String>>, adapter: &str, model: &str) -> bool {
    allowed.as_ref().is_none_or(|a| a.contains(&format!("{adapter}/{model}")))
}
