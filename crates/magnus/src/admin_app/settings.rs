//! What an admin sets for the whole workspace in the app's Admin section and
//! the kernel keeps (spec rev 141, Administration): the workspace rules that
//! cap every person's own rules, and whether Auto Review acts for the
//! workspace, and the update channel the server follows. One event kind, `admin_setting`, carries each change.

use crate::rules::Effect;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One change to the workspace's settings. A retired form keeps its reader.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    /// A workspace rule written or changed: what it matches and what it does.
    Rule {
        id: String,
        #[serde(default)]
        rule: WorkspaceRule,
    },
    RuleRemoved {
        id: String,
    },
    /// Whether Auto Review may clear an action alone for the workspace.
    AutoReview {
        acts: bool,
    },
    /// The update channel the server itself follows (`server_channel.rs`).
    ServerChannel {
        channel: String,
    },
}

/// A rule an admin set for everyone: the same match a person's own rule has
/// (a connector, a tool, destinations), compiled for each person it is
/// checked for. A block or an ask first here takes in a person's action
/// whatever their own rules allow, because block wins over both and ask
/// first over allow (spec rev 140, Rules): that is the cap.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct WorkspaceRule {
    pub effect: Effect,
    pub connector: String,
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub destinations: Vec<String>,
    /// The admin's own words for it.
    #[serde(default)]
    pub words: Option<String>,
    #[serde(default)]
    pub by: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Settings {
    pub rules: BTreeMap<String, WorkspaceRule>,
    /// None: as the config's `[auto_review] delegate` says.
    pub auto_review: Option<bool>,
    /// None: production.
    pub server_channel: Option<String>,
}

impl Settings {
    pub fn apply(&mut self, c: &Change) {
        match c {
            Change::Rule { id, rule } => {
                self.rules.insert(id.clone(), rule.clone());
            }
            Change::RuleRemoved { id } => {
                self.rules.remove(id);
            }
            Change::AutoReview { acts } => self.auto_review = Some(*acts),
            Change::ServerChannel { channel } => self.server_channel = Some(channel.clone()),
        }
    }
}

/// The id a workspace rule carries among a person's rules, so an ask or a
/// denial it raised names it: `workspace:<id>`.
pub const WORKSPACE_RULE: &str = "workspace:";

impl crate::kernel::Kernel {
    /// Whether Auto Review clears an action alone: the admin's setting, else
    /// the config's.
    pub fn auto_review_acts(&self) -> bool {
        self.read(|s| s.admin_settings.auto_review).unwrap_or(self.cfg.auto_review.delegate)
    }

    /// A person's rules as the boundary checks them: their own, and every
    /// workspace rule compiled for them. A workspace rule that no longer
    /// compiles covers nothing.
    pub fn rules_for(&self, person: &str) -> Vec<crate::rules::Rule> {
        let (mut own, workspace) = self.read(|s| (s.rules.of(person), s.admin_settings.rules.clone()));
        own.extend(workspace.into_iter().map(|(id, w)| workspace_rule(person, &id, &w)));
        own
    }
}

/// A workspace rule as one of `person`'s: accepted, never used up.
pub fn workspace_rule(person: &str, id: &str, w: &WorkspaceRule) -> crate::rules::Rule {
    let compiled = crate::rules::compile(person, &w.connector, &w.tool, &w.destinations, false, w.effect).ok();
    crate::rules::Rule { id: format!("{WORKSPACE_RULE}{id}"), person: person.to_string(), compiled, words: w.words.clone(), accepted: true, used: 0 }
}
