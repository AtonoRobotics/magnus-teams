//! Spend and Models administration (spec rev 141, Admin):
//!
//! "Spend. A monthly limit for the server, a default limit per person,
//! overrides for individuals, and limits per channel for the team agent.
//! Limits cascade, so no person exceeds the server's limit. Admins are
//! alerted at 75% and 90% of a limit. Spend is broken down by person,
//! agent, model and project. A person who reaches their limit can ask for
//! more from inside the app, and an admin approves or declines."
//!
//! "Models. The providers and models the server offers, the default model,
//! and which models each role or person may use, so costly models can be
//! limited to the people who need them."
//!
//! What admins set travels as `spend` events and is read back here
//! (decisions/2026-11-11-spend-and-models.md). The team agent's workspace
//! and channel limits stay the team's own `spend_limit` change
//! (`team::spend`); this module reads them for the cascade, the alerts and
//! the breakdowns, and Admin → Spend sets them through the same change.

pub mod access;
pub mod admin;
pub mod alerts;
pub mod enforce;
pub mod month;
pub mod requests;
pub mod routes;

#[cfg(test)]
mod tests;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The largest monthly limit an admin can set, in dollars: past it, a typo.
pub const MAX_LIMIT_USD: f64 = crate::team::spend::MAX_LIMIT_USD;

/// The shares of a limit at which admins are alerted, in percent.
pub const ALERT_PERCENTS: [u8; 2] = [75, 90];

/// How often the alerts are checked, in seconds.
pub const ALERT_CHECK_INTERVAL_SECS: u64 = 60;

/// Whose limit a `LimitSet` changes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// The whole server's monthly limit.
    Server,
    /// What each person may spend unless they have an override.
    Default,
    /// One person's override.
    Person(String),
}

/// What admins and people did about spend and model access. A kind is
/// never edited or deleted; a new shape is a new variant.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    /// A monthly limit in dollars was set, or lifted (None).
    LimitSet { scope: Scope, usd: Option<f64> },
    /// Admins were told that a limit's spend this month reached `percent`.
    /// `limit` names it: `server`, `person:<id>`, `workspace:<ws>` or
    /// `channel:<ws>/<channel>`.
    Alerted { limit: String, month: String, percent: u8, spent_usd: f64, limit_usd: f64 },
    /// A person at their limit asked for more this month.
    IncreaseAsked {
        id: String,
        person: String,
        month: String,
        usd: f64,
        #[serde(default)]
        reason: String,
    },
    /// An admin approved (raising the person's limit for that month by
    /// `usd`) or declined a request.
    IncreaseDecided {
        id: String,
        approved: bool,
        usd: f64,
        by: String,
        #[serde(default)]
        note: String,
    },
    /// The models a role or a person may use (`adapter/model`), or the rule
    /// removed (None). A person's rule wins over their roles'.
    ModelsAllowed {
        #[serde(default)]
        role: Option<String>,
        #[serde(default)]
        person: Option<String>,
        models: Option<Vec<String>>,
    },
}

/// A person's request for more spend.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Request {
    pub id: String,
    pub person: String,
    pub month: String,
    pub usd: f64,
    pub reason: String,
    pub at: DateTime<Utc>,
    /// None while it waits; then (approved, dollars granted, by whom, note, when).
    pub decided: Option<Decided>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Decided {
    pub approved: bool,
    pub usd: f64,
    pub by: String,
    pub note: String,
    pub at: DateTime<Utc>,
}

/// The state the `spend` events build.
#[derive(Clone, Debug, Default)]
pub struct Ledger {
    pub server_usd: Option<f64>,
    pub default_usd: Option<f64>,
    pub people_usd: BTreeMap<String, f64>,
    /// Approved increases: (person, month) to the dollars added.
    pub extra_usd: BTreeMap<(String, String), f64>,
    /// (limit, month, percent) already alerted.
    pub alerted: BTreeSet<(String, String, u8)>,
    pub requests: BTreeMap<String, Request>,
    pub access: access::Access,
}

/// Their limit, increases, alerts, requests (whose reasons are their
/// words) and model access. Decisions they made on others' requests stay
/// with those requests.
impl crate::forget::Forget for Ledger {
    fn forget(&mut self, who: &crate::forget::Erased) {
        let p = &who.person;
        let limit = format!("person:{p}");
        self.people_usd.remove(p);
        self.extra_usd.retain(|(x, _), _| x != p);
        self.alerted.retain(|(l, _, _)| *l != limit);
        self.requests.retain(|_, r| r.person != *p);
        self.access.people.remove(p);
    }
}

impl Ledger {
    pub fn apply(&mut self, at: DateTime<Utc>, c: &Change) {
        match c {
            Change::LimitSet { scope, usd } => match scope {
                Scope::Server => self.server_usd = *usd,
                Scope::Default => self.default_usd = *usd,
                Scope::Person(p) => {
                    match usd {
                        Some(u) => self.people_usd.insert(p.clone(), *u),
                        None => self.people_usd.remove(p),
                    };
                }
            },
            Change::Alerted { limit, month, percent, .. } => {
                self.alerted.insert((limit.clone(), month.clone(), *percent));
            }
            Change::IncreaseAsked { id, person, month, usd, reason } => {
                let r = Request { id: id.clone(), person: person.clone(), month: month.clone(), usd: *usd, reason: reason.clone(), at, decided: None };
                self.requests.insert(id.clone(), r);
            }
            Change::IncreaseDecided { id, approved, usd, by, note } => {
                let Some(r) = self.requests.get_mut(id) else { return };
                if r.decided.is_some() {
                    return;
                }
                r.decided = Some(Decided { approved: *approved, usd: *usd, by: by.clone(), note: note.clone(), at });
                if *approved {
                    *self.extra_usd.entry((r.person.clone(), r.month.clone())).or_default() += usd;
                }
            }
            Change::ModelsAllowed { role, person, models } => self.access.set(role.as_deref(), person.as_deref(), models.clone()),
        }
    }

    /// A person's own monthly limit for `month` before the cascade: their
    /// override or the default, plus what admins approved that month.
    pub fn person_limit(&self, person: &str, month: &str) -> Option<f64> {
        let base = self.people_usd.get(person).copied().or(self.default_usd)?;
        Some(base + self.extra_usd.get(&(person.to_string(), month.to_string())).copied().unwrap_or_default())
    }

    /// The person's request still waiting for `month`, if any.
    pub fn pending(&self, person: &str, month: &str) -> Option<&Request> {
        self.requests.values().find(|r| r.person == person && r.month == month && r.decided.is_none())
    }
}

/// Whether an amount is one an admin may set as a limit or a grant.
pub fn valid_usd(usd: f64) -> bool {
    usd.is_finite() && (0.0..=MAX_LIMIT_USD).contains(&usd)
}

/// Starts the alert check: once now, then every `ALERT_CHECK_INTERVAL_SECS`.
pub fn schedule(k: std::sync::Arc<crate::kernel::Kernel>) {
    tokio::spawn(async move {
        let mut t = tokio::time::interval(std::time::Duration::from_secs(ALERT_CHECK_INTERVAL_SECS));
        loop {
            t.tick().await;
            k.spend_alerts(Utc::now()).await;
        }
    });
}
