//! Who people are here, as the log has it: their sign-ins, the identity
//! provider account each is bound to, the groups and roles from the
//! provider and from the owner, whether people's data is private, each
//! person's own settings and their home mark.
//!
//! These are folded facts only. What a person may do is answered by
//! `people::rights` alone, which reads them.

use crate::cadence::Zone;
use crate::events::Ev;
use crate::forget::Erased;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap, HashSet};

/// Groups and roles from the identity provider, as of a sign-in.
pub type ProviderRights = (Option<Vec<String>>, Option<Vec<String>>);

#[derive(Default, Debug)]
pub struct Identity {
    /// Everyone who has ever signed in here. Someone not in it signs in the
    /// first time on the identity provider's own page, which can walk them
    /// through what their account still needs (a new password, two-factor).
    signed_in_before: HashSet<String>,
    /// When each person last signed in here (the console's Users list).
    last_sign_in: HashMap<String, DateTime<Utc>>,
    /// When each person last opened the app's home: the feed starts
    /// here (#367).
    home_last_open: HashMap<String, DateTime<Utc>>,
    /// People turned off at the identity provider or by an admin, until
    /// they sign in again: their apps answer no calls for them.
    turned_off: BTreeSet<String>,
    /// Groups and roles from the identity provider, per person, as of their
    /// latest sign-in.
    provider_rights: HashMap<String, ProviderRights>,
    /// The identity provider account (`sub`) each person is bound to.
    subjects: HashMap<String, String>,
    /// Roles the owner set in Settings, by person.
    roles_set: HashMap<String, Vec<String>>,
    /// Whether people's data is private from the admin (by default it is not).
    data_private: bool,
    /// Each person's own settings, by person.
    preferences: HashMap<String, Value>,
}

impl crate::forget::Forget for Identity {
    fn forget(&mut self, who: &Erased) {
        let p = who.person.as_str();
        self.turned_off.remove(p);
        self.preferences.remove(p);
        self.subjects.remove(p);
        self.last_sign_in.remove(p);
        self.signed_in_before.remove(p);
        // Their home mark: whoever is next known by their name starts
        // fresh (#412).
        self.home_last_open.remove(p);
        // The rights they held go with them: whoever is next known by their
        // name starts from the organisation's config.
        self.roles_set.remove(p);
        self.provider_rights.remove(p);
        // Kept: whether people's data is private is the organisation's.
    }
}

impl Identity {
    pub(crate) fn fold(&mut self, ev: &Ev, at: DateTime<Utc>) {
        match ev {
            Ev::SubjectBound { person, subject } => match subject {
                Some(sub) => {
                    self.subjects.insert(person.clone(), sub.clone());
                }
                None => {
                    self.subjects.remove(person);
                }
            },
            Ev::RightsFromProvider { person, groups, roles } => {
                self.provider_rights.insert(person.clone(), (groups.clone(), roles.clone()));
            }
            Ev::DataAccessSet { private } => self.data_private = *private,
            Ev::RolesSet { person, roles } => {
                self.roles_set.insert(person.clone(), roles.clone());
            }
            Ev::PreferencesSet { person, prefs } => {
                let p = self.preferences.entry(person.clone()).or_insert_with(|| serde_json::json!({}));
                if let (Some(p), Some(new)) = (p.as_object_mut(), prefs.as_object()) {
                    for (k, v) in new {
                        p.insert(k.clone(), v.clone());
                    }
                }
            }
            Ev::SignedIn { person, .. } => {
                self.signed_in_before.insert(person.clone());
                self.last_sign_in.insert(person.clone(), at);
                // Signed in again: the provider has them back.
                self.turned_off.remove(person);
            }
            Ev::HomeOpened { person } => {
                self.home_last_open.insert(person.clone(), at);
            }
            Ev::PersonTurnedOff { person, .. } => {
                self.turned_off.insert(person.clone());
            }
            _ => {}
        }
    }

    pub fn signed_in_before(&self, person: &str) -> bool {
        self.signed_in_before.contains(person)
    }

    pub fn last_sign_in(&self, person: &str) -> Option<DateTime<Utc>> {
        self.last_sign_in.get(person).copied()
    }

    pub fn home_last_open(&self, person: &str) -> Option<DateTime<Utc>> {
        self.home_last_open.get(person).copied()
    }

    pub fn is_turned_off(&self, person: &str) -> bool {
        self.turned_off.contains(person)
    }

    pub fn provider_rights(&self, person: &str) -> Option<&ProviderRights> {
        self.provider_rights.get(person)
    }

    /// Whether the identity provider set the person's roles.
    pub fn provider_set_roles(&self, person: &str) -> bool {
        self.provider_rights.get(person).is_some_and(|(_, r)| r.is_some())
    }

    /// The identity provider account the person is bound to.
    pub fn subject(&self, person: &str) -> Option<&str> {
        self.subjects.get(person).map(String::as_str)
    }

    /// Every binding, person to subject.
    pub fn subjects(&self) -> &HashMap<String, String> {
        &self.subjects
    }

    pub fn roles_set(&self, person: &str) -> Option<&Vec<String>> {
        self.roles_set.get(person)
    }

    pub fn data_private(&self) -> bool {
        self.data_private
    }

    /// Everyone's own settings, by person.
    pub fn preferences(&self) -> &HashMap<String, Value> {
        &self.preferences
    }

    pub fn preferences_of(&self, person: &str) -> Option<&Value> {
        self.preferences.get(person)
    }

    /// The person's zone, from their preferences.
    pub fn zone(&self, person: &str) -> Zone {
        crate::cadence::zone_of(self.preferences.get(person))
    }

    #[cfg(test)]
    pub(crate) fn has_home_marks(&self) -> bool {
        !self.home_last_open.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn canonical_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("signed_in_before", format!("{:?}", self.signed_in_before)),
            ("last_sign_in", format!("{:?}", self.last_sign_in)),
            ("home_last_open", format!("{:?}", self.home_last_open)),
            ("turned_off", format!("{:?}", self.turned_off)),
            ("provider_rights", format!("{:?}", self.provider_rights)),
            ("subjects", format!("{:?}", self.subjects)),
            ("roles_set", format!("{:?}", self.roles_set)),
            ("data_private", format!("{:?}", self.data_private)),
            ("preferences", format!("{:?}", self.preferences)),
        ]
    }
}
