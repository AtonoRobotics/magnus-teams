//! The organisation's apps (connectors) as the log records them: those the
//! owner added, admins' settings for each, the credentials and provider
//! registrations set (never their values), and how often each person
//! connected each app.

use crate::config::Connector;
use crate::events::Ev;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// An admin's settings for one of the organisation's apps: on or off, and
/// who may use it (None: the app's own groups).
#[derive(Clone, Debug, Default)]
pub struct ConnectorSetting {
    pub enabled: bool,
    pub access_groups: Option<Vec<String>>,
    pub blocked_groups: Option<Vec<String>>,
}

#[derive(Default, Debug)]
pub struct Apps {
    /// Apps the owner added from the app (config-file apps are not here).
    connectors: BTreeMap<String, Connector>,
    /// The admin's settings for each app: on or off, and who may use it.
    connector_settings: BTreeMap<String, ConnectorSetting>,
    /// How many times each person has connected each app (by person, then
    /// app): the first connection is the one Magnus opens a conversation
    /// about, never a reconnection.
    app_connections: HashMap<(String, String), u32>,
    /// When each organisation credential was last set or revoked: an app's
    /// key by the app's name, a provider's client secret as
    /// `registration:<provider>`. Never the value.
    credential_changed: BTreeMap<String, (bool, DateTime<Utc>)>,
    /// Providers the organisation has a registration with (its client ID
    /// and secret are in the vault).
    registrations: BTreeSet<String>,
}

/// Their connections' count. The organisation's apps, settings and
/// credentials are the organisation's; a generated app of theirs is harmless
/// once their id is never given again
/// (2026-11-11-an-erased-persons-id-is-not-given-again.md).
impl crate::forget::Forget for Apps {
    fn forget(&mut self, who: &crate::forget::Erased) {
        self.app_connections.retain(|(p, _), _| !who.is(p));
    }
}

impl Apps {
    /// `ConnectorAdded`, `ConnectorSettings`, `AppConnected`,
    /// `AppDisconnected`, `CredentialSet`, `ConnectorRemoved`,
    /// `RegistrationSet` and `ToolClassified`. `State::apply` routes no
    /// other kind here.
    pub(crate) fn fold(&mut self, ev: &Ev, at: DateTime<Utc>) {
        match ev {
            Ev::ConnectorAdded { connector } => {
                if connector.credential.is_some() && connector.auth.as_deref() != Some(crate::connectors::OAUTH) {
                    self.credential_changed.insert(connector.name.clone(), (true, at));
                }
                self.connectors.insert(connector.name.clone(), connector.clone());
            }
            Ev::ConnectorSettings { name, enabled, access_groups, blocked_groups } => {
                self.connector_settings.insert(name.clone(), ConnectorSetting { enabled: *enabled, access_groups: access_groups.clone(), blocked_groups: blocked_groups.clone() });
            }
            // The connection itself is in the vault; these are its record.
            Ev::AppConnected { app, person, .. } => {
                *self.app_connections.entry((person.clone(), app.clone())).or_default() += 1;
            }
            Ev::AppDisconnected { .. } => {}
            Ev::CredentialSet { connector, set } => {
                self.credential_changed.insert(connector.clone(), (*set, at));
            }
            Ev::ConnectorRemoved { name } => {
                self.connectors.remove(name);
            }
            Ev::RegistrationSet { provider, set } => {
                self.credential_changed.insert(format!("registration:{provider}"), (*set, at));
                if *set {
                    self.registrations.insert(provider.clone());
                } else {
                    self.registrations.remove(provider);
                }
            }
            Ev::ToolClassified { connector, tool, risk } => {
                if let Some(c) = self.connectors.get_mut(connector) {
                    match risk {
                        Some(r) => {
                            // Its reach is what the tool is, whatever its class.
                            let reach = c.tools.get(tool).and_then(|t| t.reach.clone());
                            c.tools.insert(tool.clone(), crate::config::ToolClass { risk: r.clone(), label: None, reach });
                        }
                        None => {
                            c.tools.remove(tool);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// The apps the owner added, by name.
    pub fn connectors(&self) -> &BTreeMap<String, Connector> {
        &self.connectors
    }

    pub fn connector_settings(&self) -> &BTreeMap<String, ConnectorSetting> {
        &self.connector_settings
    }

    /// How many times the person has connected the app.
    pub fn connections(&self, person: &str, app: &str) -> Option<u32> {
        self.app_connections.get(&(person.to_string(), app.to_string())).copied()
    }

    pub fn credential_changed(&self) -> &BTreeMap<String, (bool, DateTime<Utc>)> {
        &self.credential_changed
    }

    pub fn registrations(&self) -> &BTreeSet<String> {
        &self.registrations
    }

    #[cfg(test)]
    pub(crate) fn canonical_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("connectors", format!("{:?}", self.connectors)),
            ("connector_settings", format!("{:?}", self.connector_settings)),
            ("app_connections", format!("{:?}", self.app_connections)),
            ("credential_changed", format!("{:?}", self.credential_changed)),
            ("registrations", format!("{:?}", self.registrations)),
        ]
    }
}
