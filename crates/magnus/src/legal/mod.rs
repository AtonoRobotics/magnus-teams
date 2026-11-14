//! The agreements between each person and whoever runs this server (owner,
//! 2026-11-11): the Terms of Use and the Privacy notice everyone accepts
//! before using the app, the choice to share anonymous usage data, and the
//! tester program, joined only by accepting the Tester Terms.
//!
//! Each acceptance and choice is a `legal` event naming the version of the
//! text the person was shown. A version is the text's date and a digest of
//! the text as this server renders it (`documents`), so a change to the
//! text or to the `[legal]` config it names asks everyone again.
//!
//! What the choices change is in `crate::anonymize`: a developer view shows
//! a tester's work in full, a sharer's anonymised, and nothing of anyone
//! else's.

pub mod documents;
pub mod routes;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The `[legal]` config: the facts about who runs this server that the
/// agreements state and the code cannot know. Every one is required.
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// The legal name of whoever runs this server: "Magnus's developers".
    #[serde(default)]
    pub operator: String,
    /// Where people write about their account, their data and these terms.
    #[serde(default)]
    pub contact: String,
    /// The law the Terms of Use are under, for example "the State of
    /// California, United States".
    #[serde(default)]
    pub governing_law: String,
    /// The most days the developers keep anything they copied out of a
    /// tester's data to fix a bug (a bug report, a log excerpt). The code
    /// keeps no such copy itself, so this is the operator's promise.
    #[serde(default)]
    pub tester_copy_days: u32,
}

impl Settings {
    /// Was a `[legal]` section given at all? Without one the server runs as
    /// before the agreements: nobody is asked, nobody can accept or join
    /// the tester program, and developer views show nobody else's runs.
    pub fn is_set(&self) -> bool {
        !(self.operator.trim().is_empty() && self.contact.trim().is_empty() && self.governing_law.trim().is_empty() && self.tester_copy_days == 0)
    }

    /// Startup refuses a server whose agreements would be missing a fact.
    pub fn check(&self) -> anyhow::Result<()> {
        let mut missing = vec![];
        if self.operator.trim().is_empty() {
            missing.push("operator = \"<the legal name of whoever runs this server>\"");
        }
        if !self.contact.contains('@') || self.contact.trim().contains(char::is_whitespace) {
            missing.push("contact = \"<the email address people write to about their data>\"");
        }
        if self.governing_law.trim().is_empty() {
            missing.push("governing_law = \"<the law the Terms of Use are under>\"");
        }
        if self.tester_copy_days == 0 {
            missing.push("tester_copy_days = <the most days developers keep what they copy from a tester's data>");
        }
        anyhow::ensure!(
            missing.is_empty(),
            "[legal] is incomplete: the Terms of Use, Privacy notice and Tester Terms state these facts and Magnus cannot know them. Add to the config:\n[legal]\n{}",
            missing.join("\n")
        );
        Ok(())
    }
}

/// A complete `[legal]` section for test configs.
#[cfg(test)]
pub const TEST_CONFIG: &str = "[legal]\noperator = \"Example Ltd\"\ncontact = \"magnus@example.com\"\ngoverning_law = \"England and Wales\"\ntester_copy_days = 90\n";

/// The group testers are in. Nobody is put in it by the config or an
/// identity provider: only accepting the Tester Terms joins it.
pub const TESTERS_GROUP: &str = "testers";

/// Put `p` in [`TESTERS_GROUP`] exactly when they are a tester of the
/// Tester Terms as they read now.
pub fn testers_group(k: &crate::kernel::Kernel, p: &mut crate::config::Person) {
    p.groups.retain(|g| g != TESTERS_GROUP);
    let now = documents::Versions::of(&k.cfg.legal);
    if k.read(|s| s.legal.tester_of(&p.id).is_some_and(|v| *v == now.testers)) {
        p.groups.push(TESTERS_GROUP.into());
    }
}

/// One change to a person's agreements, with the version of the text shown.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "what", rename_all = "snake_case")]
pub enum Change {
    /// The Terms of Use accepted, with the Privacy notice shown beside them.
    TermsAccepted { person: String, terms: String, privacy: String },
    /// Yes or no to sharing anonymous usage data, from the prompt's text `version`.
    UsageChosen { person: String, share: bool, version: String },
    /// An admin asked them to join the tester program.
    TesterInvited { person: String, by: String },
    /// They declined the invitation.
    TesterDeclined { person: String },
    /// They accepted the Tester Terms of `version`.
    TesterJoined { person: String, version: String },
    /// They left the program; `version` is the Tester Terms they had accepted.
    TesterLeft { person: String, version: String },
}

/// Everyone's agreements as they stand.
#[derive(Default, Clone, Debug)]
pub struct Ledger {
    /// Person, then the Terms and Privacy versions they accepted.
    terms: BTreeMap<String, (String, String)>,
    /// Person, then their usage choice and the prompt version it answered.
    usage: BTreeMap<String, (bool, String)>,
    /// Testers, with the Tester Terms version each accepted.
    testers: BTreeMap<String, String>,
    /// Invited and not yet answered, with who invited them.
    invited: BTreeMap<String, String>,
}

/// Their agreements and choices as they stand. The log keeps each
/// acceptance's record, which names versions only.
impl crate::forget::Forget for Ledger {
    fn forget(&mut self, who: &crate::forget::Erased) {
        let p = &who.person;
        self.terms.remove(p);
        self.usage.remove(p);
        self.testers.remove(p);
        self.invited.remove(p);
    }
}

impl Ledger {
    pub fn apply(&mut self, c: &Change) {
        match c {
            Change::TermsAccepted { person, terms, privacy } => {
                self.terms.insert(person.clone(), (terms.clone(), privacy.clone()));
            }
            Change::UsageChosen { person, share, version } => {
                self.usage.insert(person.clone(), (*share, version.clone()));
            }
            Change::TesterInvited { person, by } => {
                self.invited.insert(person.clone(), by.clone());
            }
            Change::TesterDeclined { person } => {
                self.invited.remove(person);
            }
            Change::TesterJoined { person, version } => {
                self.invited.remove(person);
                self.testers.insert(person.clone(), version.clone());
            }
            Change::TesterLeft { person, .. } => {
                self.testers.remove(person);
            }
        }
    }

    /// The Terms and Privacy versions `person` accepted.
    pub fn terms_of(&self, person: &str) -> Option<&(String, String)> {
        self.terms.get(person)
    }

    /// Their usage choice and the prompt version it answered.
    pub fn usage_of(&self, person: &str) -> Option<&(bool, String)> {
        self.usage.get(person)
    }

    /// The Tester Terms version they accepted, while they are a tester.
    pub fn tester_of(&self, person: &str) -> Option<&String> {
        self.testers.get(person)
    }

    pub fn invited_by(&self, person: &str) -> Option<&String> {
        self.invited.get(person)
    }

    /// Every tester and the version each accepted.
    pub fn testers(&self) -> &BTreeMap<String, String> {
        &self.testers
    }

    pub fn invited(&self) -> &BTreeMap<String, String> {
        &self.invited
    }

    /// Testers of the Tester Terms as they read now. Someone who accepted
    /// an earlier text is not seen in full until they accept this one.
    pub fn testers_now(&self, now: &documents::Versions) -> BTreeSet<String> {
        self.testers.iter().filter(|(_, v)| **v == now.testers).map(|(p, _)| p.clone()).collect()
    }

    /// Those whose yes answered the prompt as it reads now.
    pub fn sharing_now(&self, now: &documents::Versions) -> BTreeSet<String> {
        self.usage.iter().filter(|(_, (share, v))| *share && *v == now.usage).map(|(p, _)| p.clone()).collect()
    }
}
