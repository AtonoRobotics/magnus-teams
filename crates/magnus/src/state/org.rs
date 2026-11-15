//! The organisation's settings as admins set them: policy layers and their
//! drafts and history, run modules, model providers, the model, the desktop
//! channel, the console's session lifetimes, adapters' conformance, the
//! admin audit and the version notices (U5).

use crate::events::Ev;
use crate::log::Event;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Debug)]
pub struct PolicyVersion {
    pub layer: String,
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub by: String,
    pub cedar: String,
}

/// A conformance result: version, model, whether it passed, and the report.
pub type Conformance = (String, String, bool, Value);
/// An audit entry: its seq, when, and the action as written.
pub type AuditEntry = (i64, DateTime<Utc>, Value);
/// A version notice: seq, when, what changed, and its new value.
pub type VersionChange = (i64, DateTime<Utc>, String, String);

#[derive(Default, Debug)]
pub struct OrgSettings {
    policies: BTreeMap<String, String>,
    /// Every policy a layer ever had, oldest first.
    policy_history: Vec<PolicyVersion>,
    /// Admins' drafts of policy layers, not in force.
    policy_drafts: BTreeMap<String, String>,
    /// Run modules an admin turned on or off for the organisation.
    module_settings: BTreeMap<String, bool>,
    /// Model providers an admin disabled.
    providers_off: BTreeSet<String>,
    /// The desktop channel for everyone but the owner and testers, when an
    /// admin set one.
    desktop_channel: Option<String>,
    /// The admin console's session lifetimes, as an admin set them.
    console_settings: Option<Value>,
    /// The model new tasks run on: adapter and model.
    model: Option<(String, String)>,
    /// Latest conformance result per adapter id.
    conformance: HashMap<String, Conformance>,
    /// What was done in the admin console, oldest first.
    audit: Vec<AuditEntry>,
    /// Model and harness changes, for version notices (U5).
    version_changes: Vec<VersionChange>,
    /// What an admin set for the whole workspace in the Admin section: its
    /// rules, Auto Review, the server's channel, retention and the identity
    /// provider.
    admin: crate::admin_app::settings::Settings,
}

/// The organisation's own settings and record are nobody's: a policy layer
/// naming a person is the admins' rule, and the audit is the record of
/// what was done, the erasure among it (#195).
impl crate::forget::Forget for OrgSettings {
    fn forget(&mut self, _: &crate::forget::Erased) {}
}

impl OrgSettings {
    pub(crate) fn fold_admin(&mut self, c: &crate::admin_app::settings::Change) {
        self.admin.apply(c);
    }

    /// What an admin set for the whole workspace.
    pub fn admin(&self) -> &crate::admin_app::settings::Settings {
        &self.admin
    }

    /// The policy, module, provider, channel, console, model, adapter,
    /// boot and admin action kinds. `State::apply` routes no other kind here.
    pub(crate) fn fold(&mut self, e: &Event) {
        match &e.ev {
            Ev::KernelBoot { version, instructions, .. } => {
                for (what, v) in [("harness", version), ("instructions", instructions)] {
                    if !v.is_empty() && self.version_changes.iter().rev().find(|c| c.2 == what).is_none_or(|c| &c.3 != v) {
                        self.note_version(e, what, v.clone());
                    }
                }
            }
            Ev::PolicySet { layer, cedar } => {
                self.policies.insert(layer.clone(), cedar.clone());
                self.policy_history.push(PolicyVersion { layer: layer.clone(), seq: e.seq, at: e.at, by: e.actor.clone(), cedar: cedar.clone() });
                let n = self.policy_history.iter().filter(|p| &p.layer == layer).count();
                self.note_version(e, &format!("policy:{layer}"), format!("v{n}"));
            }
            Ev::ModuleSet { name, on } => match on {
                Some(on) => {
                    self.module_settings.insert(name.clone(), *on);
                }
                None => {
                    self.module_settings.remove(name);
                }
            },
            Ev::ProviderSet { id, enabled } => {
                if *enabled {
                    self.providers_off.remove(id);
                } else {
                    self.providers_off.insert(id.clone());
                }
            }
            Ev::DesktopChannelSet { channel } => {
                self.desktop_channel = Some(channel.clone());
            }
            Ev::ConsoleSettings { idle_minutes, session_hours, allow } => {
                self.console_settings = Some(serde_json::json!({ "idle_minutes": idle_minutes, "session_hours": session_hours, "allow": allow }));
            }
            Ev::PolicyDrafted { layer, cedar } => match cedar {
                Some(c) => {
                    self.policy_drafts.insert(layer.clone(), c.clone());
                }
                None => {
                    self.policy_drafts.remove(layer);
                }
            },
            Ev::AdminAction { .. } => {
                if let Ok(v) = serde_json::to_value(&e.ev) {
                    self.note_audit(e, v);
                }
            }
            Ev::ModelSet { adapter, model } => {
                self.model = Some((adapter.clone(), model.clone()));
                self.note_version(e, "model", format!("{adapter}/{model}"));
            }
            Ev::AdapterVerified { adapter_id, version, model, passed, report, .. } => {
                self.conformance.insert(adapter_id.clone(), (version.clone(), model.clone(), *passed, report.clone()));
            }
            _ => {}
        }
    }

    /// A version notice for what changed at this event (the playbook's
    /// version among them).
    pub(crate) fn note_version(&mut self, e: &Event, what: &str, value: String) {
        self.version_changes.push((e.seq, e.at, what.into(), value));
    }

    /// An entry in the audit at this event, written by the kernel (an
    /// erasure's backups) or the console.
    pub(crate) fn note_audit(&mut self, e: &Event, action: Value) {
        self.audit.push((e.seq, e.at, action));
    }

    /// The policy layers in force, by layer.
    pub fn policies(&self) -> &BTreeMap<String, String> {
        &self.policies
    }

    pub fn policy_history(&self) -> &Vec<PolicyVersion> {
        &self.policy_history
    }

    pub fn policy_drafts(&self) -> &BTreeMap<String, String> {
        &self.policy_drafts
    }

    pub fn module_settings(&self) -> &BTreeMap<String, bool> {
        &self.module_settings
    }

    pub fn providers_off(&self) -> &BTreeSet<String> {
        &self.providers_off
    }

    pub fn desktop_channel(&self) -> &Option<String> {
        &self.desktop_channel
    }

    pub fn console_settings(&self) -> &Option<Value> {
        &self.console_settings
    }

    pub fn model(&self) -> &Option<(String, String)> {
        &self.model
    }

    pub fn conformance(&self) -> &HashMap<String, Conformance> {
        &self.conformance
    }

    pub fn audit(&self) -> &Vec<AuditEntry> {
        &self.audit
    }

    pub fn version_changes(&self) -> &Vec<VersionChange> {
        &self.version_changes
    }

    #[cfg(test)]
    pub(crate) fn canonical_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("admin_settings", format!("{:?}", self.admin)),
            ("policies", format!("{:?}", self.policies)),
            ("policy_history", format!("{:?}", self.policy_history)),
            ("policy_drafts", format!("{:?}", self.policy_drafts)),
            ("module_settings", format!("{:?}", self.module_settings)),
            ("providers_off", format!("{:?}", self.providers_off)),
            ("desktop_channel", format!("{:?}", self.desktop_channel)),
            ("console_settings", format!("{:?}", self.console_settings)),
            ("model", format!("{:?}", self.model)),
            ("conformance", format!("{:?}", self.conformance)),
            ("audit", format!("{:?}", self.audit)),
            ("version_changes", format!("{:?}", self.version_changes)),
        ]
    }
}
