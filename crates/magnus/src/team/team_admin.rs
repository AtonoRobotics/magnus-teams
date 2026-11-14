//! Team Admin (the owner, 2026-11-11: "Create Team Admin group … Team Admin
//! owns the workspace"). A Team Admin is a server role a Magnus admin
//! grants; only a Team Admin starts a workspace, and the one who starts it
//! owns it. Inside a workspace, its Team Admins are its owner and anyone
//! that owner makes an admin there (`Role::Owner`, `Role::Admin`): they
//! invite and remove members, moderate and delete what is in it, and delete
//! it. A Magnus admin grants the role and reads nothing of any workspace.
//!
//! The role is kept by the team ledger (`Change::TeamAdmin`) rather than
//! beside the four server roles, so an identity provider that names a
//! person's roles does not drop it; a provider may also name it
//! (`team-admin`). When it came in, everyone who owned a workspace became
//! one, and so did the server's owners, once (`Change::TeamAdminsSeeded`).

use super::{Change, Ledger, Role};
use crate::config::Person;
use crate::events::Ev;
use crate::kernel::Kernel;
use std::collections::BTreeSet;

/// The role's name, as a server role and as an identity provider names it.
pub const ROLE: &str = "team-admin";

/// Who is a Team Admin, and whether the first ones were made.
#[derive(Default, Clone, Debug)]
pub struct Admins {
    people: BTreeSet<String>,
    seeded: bool,
}

impl Admins {
    /// An erased person is no Team Admin.
    pub(super) fn forget(&mut self, person: &str) {
        self.people.remove(person);
    }
}

impl Admins {
    /// Apply a Team Admin change; false when `c` is not one.
    pub(super) fn apply(&mut self, c: &Change) -> bool {
        match c {
            Change::TeamAdmin { person, on: true } => {
                self.people.insert(person.clone());
            }
            Change::TeamAdmin { person, on: false } => {
                self.people.remove(person);
            }
            Change::TeamAdminsSeeded { people } => {
                self.seeded = true;
                self.people.extend(people.iter().cloned());
            }
            _ => return false,
        }
        true
    }

    pub fn holds(&self, person: &str) -> bool {
        self.people.contains(person)
    }

    pub fn all(&self) -> Vec<String> {
        self.people.iter().cloned().collect()
    }
}

impl Ledger {
    /// Everyone who owns a workspace now.
    fn owners_now(&self) -> Vec<String> {
        let mut v: BTreeSet<String> = BTreeSet::new();
        for w in self.workspaces.values() {
            v.extend(w.members.iter().filter(|(_, r)| **r == Role::Owner).map(|(p, _)| p.clone()));
        }
        v.into_iter().collect()
    }
}

impl Kernel {
    /// Whether `p` may start a workspace.
    pub fn is_team_admin(&self, p: &Person) -> bool {
        p.has_role(ROLE) || self.read(|s| s.team.admins.holds(&p.id))
    }

    /// Once: whoever owns a workspace when Team Admin comes in becomes one,
    /// and so does the server's owner, who started workspaces until now and
    /// could grant it to themselves; a Magnus admin may take it back.
    pub async fn seed_team_admins(&self) -> anyhow::Result<()> {
        let Some(mut people) = self.read(|s| (!s.team.admins.seeded).then(|| s.team.owners_now())) else { return Ok(()) };
        people.extend(self.active_owners().await);
        people.sort();
        people.dedup();
        self.emit("kernel", Ev::Team(Change::TeamAdminsSeeded { people })).await?;
        Ok(())
    }

    /// Make `person` a Team Admin or take it back, as `actor`. The
    /// workspaces they own stay theirs either way.
    pub async fn set_team_admin(&self, actor: &str, person: &str, on: bool) -> anyhow::Result<()> {
        self.emit(actor, Ev::Team(Change::TeamAdmin { person: person.to_string(), on })).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Those who own a workspace when Team Admin comes in become Team
    /// Admins, once: a role taken back later is not given again.
    #[test]
    fn the_first_team_admins_are_the_owners_of_the_day_once() {
        let mut l = Ledger::default();
        l.apply(&Change::Created { workspace: "w".into(), name: "Team".into(), owner: "mo".into() });
        l.apply(&Change::Member { workspace: "w".into(), person: "lee".into(), role: Role::Admin });
        assert_eq!(l.owners_now(), vec!["mo".to_string()], "only owners count");
        assert!(!l.admins.seeded && !l.admins.holds("mo"));
        let people = l.owners_now();
        l.apply(&Change::TeamAdminsSeeded { people });
        assert!(l.admins.seeded && l.admins.holds("mo") && !l.admins.holds("lee"));
        l.apply(&Change::TeamAdmin { person: "mo".into(), on: false });
        assert!(!l.admins.holds("mo"), "a Team Admin taken back still holds it");
        l.apply(&Change::TeamAdmin { person: "lee".into(), on: true });
        assert_eq!(l.admins.all(), vec!["lee".to_string()]);
    }
}
