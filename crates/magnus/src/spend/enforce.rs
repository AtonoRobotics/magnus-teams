//! Where a run starts, the person's model access and the month's limits
//! are checked: a model they may not use is refused with the ones they
//! may, and work past a limit does not start. Work that starts gets no
//! more budget than the tightest limit has left, so a limit holds partway
//! through a run as well (as the team's channel limits do).

use super::{access, month, Ledger};
use crate::config::Budget;
use crate::kernel::Kernel;
use crate::state::State;
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;

/// Work refused because a monthly limit is spent.
#[derive(Debug, Clone, PartialEq)]
pub struct SpendLimited {
    /// `person` or `server`.
    pub which: &'static str,
    pub month: String,
    pub spent_usd: f64,
    pub limit_usd: f64,
    /// The person may ask an admin for more (their own limit stopped it
    /// and they have no request waiting).
    pub can_ask: bool,
    /// They asked already and it waits.
    pub pending: bool,
}

impl std::fmt::Display for SpendLimited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let amounts = format!("${:.2} of ${:.2}", self.spent_usd, self.limit_usd);
        match (self.which, self.pending) {
            ("person", true) => write!(f, "You have reached your spend limit for this month ({amounts}). Your request for more is waiting for an admin."),
            ("person", false) => write!(f, "You have reached your spend limit for this month ({amounts}). You can ask an admin for more."),
            _ => write!(f, "The server has reached its spend limit for this month ({amounts}). An admin can raise it."),
        }
    }
}

impl std::error::Error for SpendLimited {}

/// What the app reads about a spent limit: which, how much, and whether
/// the person can ask for more.
pub fn details(s: &SpendLimited) -> serde_json::Value {
    serde_json::json!({ "which": s.which, "month": s.month, "spent_usd": s.spent_usd, "limit_usd": s.limit_usd, "can_ask": s.can_ask, "pending": s.pending })
}

/// A model the person may not use.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelNotAllowed {
    pub model: String,
    pub allowed: Vec<String>,
}

impl std::fmt::Display for ModelNotAllowed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.allowed.is_empty() {
            return write!(f, "You may not use {}, and no model is allowed for you. Ask an admin.", self.model);
        }
        write!(f, "You may not use {}. You can use: {}.", self.model, self.allowed.join(", "))
    }
}

impl std::error::Error for ModelNotAllowed {}

/// How much more the person may spend this month: the tighter of their own
/// limit and the server's, if either is set. A spent limit is an error.
pub fn left(l: &Ledger, server_spent: f64, person_spent: f64, person: &str, month: &str) -> Result<Option<f64>, SpendLimited> {
    let theirs = l.person_limit(person, month);
    let pending = l.pending(person, month).is_some();
    let stop = |which, spent, limit| SpendLimited {
        which,
        month: month.to_string(),
        spent_usd: spent,
        limit_usd: limit,
        can_ask: which == "person" && !pending,
        pending: which == "person" && pending,
    };
    if let Some(limit) = l.server_usd {
        if server_spent >= limit {
            return Err(stop("server", server_spent, limit));
        }
    }
    if let Some(limit) = theirs {
        if person_spent >= limit {
            return Err(stop("person", person_spent, limit));
        }
    }
    let a = l.server_usd.map(|x| x - server_spent);
    let b = theirs.map(|x| x - person_spent);
    Ok(match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    })
}

/// What the person may still spend this month, read from the state.
pub fn left_now(s: &State, person: &str, now: DateTime<Utc>) -> Result<Option<f64>, SpendLimited> {
    let m = month::key(now);
    let (all, theirs) = month::spent_by(s, &m, person);
    left(&s.spend, all, theirs, person, &m)
}

/// The model a run goes to: the one the person chose, if they may use it;
/// else the server's default, or the first they may use when the default
/// is not theirs to use.
pub fn choose(allowed: &Option<BTreeSet<String>>, chosen: Option<(String, String)>, default: (String, String)) -> Result<(String, String), ModelNotAllowed> {
    let list = || allowed.iter().flatten().cloned().collect::<Vec<_>>();
    match chosen {
        Some((a, m)) if access::permits(allowed, &a, &m) => Ok((a, m)),
        Some((a, m)) => Err(ModelNotAllowed { model: format!("{a}/{m}"), allowed: list() }),
        None if access::permits(allowed, &default.0, &default.1) => Ok(default),
        None => list()
            .first()
            .and_then(|x| x.split_once('/').map(|(a, m)| (a.to_string(), m.to_string())))
            .ok_or_else(|| ModelNotAllowed { model: format!("{}/{}", default.0, default.1), allowed: vec![] }),
    }
}

impl Kernel {
    /// The person's roles as model access reads them: the kernel's, an
    /// admin of the console's, and `member`.
    pub async fn access_roles(&self, person: &str) -> Vec<String> {
        let mut roles = self.person(person).map(|p| p.roles).unwrap_or_default();
        if !self.read(|s| s.spend.access.roles.is_empty()) && self.accounts.role(person).await == crate::accounts::Role::Admin {
            roles.push("admin".into());
        }
        roles.push(access::EVERYONE.into());
        roles
    }

    /// The models the person may use, None meaning all.
    pub async fn allowed_models(&self, person: &str) -> Option<BTreeSet<String>> {
        let roles = self.access_roles(person).await;
        self.read(|s| s.spend.access.allowed(person, &roles))
    }

    /// The check a run's start makes: the model it goes to, and its budget
    /// within what the month's limits leave.
    pub async fn admit_run(&self, person: &str, chosen: Option<(String, String)>, budget: Budget) -> anyhow::Result<((String, String), Budget)> {
        let allowed = self.allowed_models(person).await;
        let model = choose(&allowed, chosen, self.default_model())?;
        let left = self.read(|s| left_now(s, person, Utc::now()))?;
        let budget = match left {
            Some(usd) => Budget { max_usd: budget.max_usd.min(usd), ..budget },
            None => budget,
        };
        Ok((model, budget))
    }

    /// Where a lost run escalates to, if the person may use it.
    pub async fn escalation_for(&self, person: &str, to: Option<String>) -> Option<String> {
        let to = to?;
        let allowed = self.allowed_models(person).await;
        let (a, m) = to.split_once('/')?;
        access::permits(&allowed, a, m).then_some(to)
    }
}
