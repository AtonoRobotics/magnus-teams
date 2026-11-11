//! Admins are alerted when a limit's spend this month reaches 75% and 90%:
//! once per limit, share and month, as a `spend` `alerted` event and a
//! notice in each admin's inbox. Every limit is watched: the server's, each
//! person's (with what admins approved this month), and the team agent's
//! workspace and channel limits.

use super::{month, Change, ALERT_PERCENTS};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::State;
use chrono::{DateTime, Utc};
use serde_json::json;

/// A limit that crossed a share and has not been alerted for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Due {
    pub limit: String,
    /// How the alert names it.
    pub label: String,
    pub month: String,
    pub percent: u8,
    pub spent_usd: f64,
    pub limit_usd: f64,
}

/// Every limit of the month with its spend: (key, label, spent, limit).
fn limits(s: &State, now: DateTime<Utc>) -> Vec<(String, String, f64, f64)> {
    let m = month::key(now);
    let spent = month::spent(s, &m);
    let mut out = vec![];
    if let Some(l) = s.spend.server_usd {
        out.push(("server".to_string(), "The server's monthly limit".to_string(), spent.total_usd, l));
    }
    let mut people: std::collections::BTreeSet<&String> = spent.by_person.keys().collect();
    people.extend(s.spend.people_usd.keys());
    for p in people {
        if let Some(l) = s.spend.person_limit(p, &m) {
            out.push((format!("person:{p}"), format!("{p}'s monthly limit"), spent.by_person.get(p).copied().unwrap_or_default(), l));
        }
    }
    for w in s.team.all() {
        let team = crate::team::spend::spent_this_month(s, &w.id, now);
        if let Some(l) = w.chat.limit_usd {
            out.push((format!("workspace:{}", w.id), format!("The team agent's limit in {}", w.name), team.total_usd, l));
        }
        for (c, l) in &w.chat.channel_limits_usd {
            let name = w.channels.get(c).map(|x| x.name.as_str()).unwrap_or(c.as_str());
            out.push((format!("channel:{}/{c}", w.id), format!("The team agent's limit in #{name} ({})", w.name), team.channels_usd.get(c).copied().unwrap_or_default(), *l));
        }
    }
    out
}

/// The alerts due now.
pub fn due(s: &State, now: DateTime<Utc>) -> Vec<Due> {
    let m = month::key(now);
    let mut out = vec![];
    for (limit, label, spent, l) in limits(s, now) {
        if l <= 0.0 {
            continue;
        }
        for p in ALERT_PERCENTS {
            if spent >= l * f64::from(p) / 100.0 && !s.spend.alerted.contains(&(limit.clone(), m.clone(), p)) {
                out.push(Due { limit: limit.clone(), label: label.clone(), month: m.clone(), percent: p, spent_usd: spent, limit_usd: l });
            }
        }
    }
    out
}

impl Kernel {
    /// Everyone who administers the server, as the app's admin guard
    /// decides it (`admin_app::admin_of`).
    pub async fn admins(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.cfg.people.iter().map(|p| p.id.clone()).collect();
        ids.extend(self.accounts.local_people().into_iter().map(|(id, _)| id));
        ids.sort();
        ids.dedup();
        let mut out = vec![];
        for id in ids {
            let Some(p) = self.person(&id) else { continue };
            if crate::admin_app::admin_of(&p, self.accounts.role(&id).await) {
                out.push(id);
            }
        }
        out
    }

    /// Records and sends the alerts due now. A failed write is tried again
    /// on the next check.
    pub async fn spend_alerts(&self, now: DateTime<Utc>) {
        let due = self.read(|s| due(s, now));
        if due.is_empty() {
            return;
        }
        let admins = self.admins().await;
        for d in due {
            let label = match d.limit.strip_prefix("person:").and_then(|id| self.person(id)) {
                Some(p) if !p.name.is_empty() => format!("{}'s monthly limit", p.name),
                _ => d.label.clone(),
            };
            let summary = format!("{label} is {}% spent: ${:.2} of ${:.2} this month.", d.percent, d.spent_usd, d.limit_usd);
            let details =
                json!({ "reason": "spend_alert", "evidence": { "limit": d.limit, "month": d.month, "percent": d.percent, "spent_usd": d.spent_usd, "limit_usd": d.limit_usd } });
            let mut evs = vec![("kernel", Ev::Spend(Change::Alerted { limit: d.limit, month: d.month, percent: d.percent, spent_usd: d.spent_usd, limit_usd: d.limit_usd }))];
            evs.extend(admins.iter().map(|a| ("kernel", self.notice_to(None, None, a.clone(), summary.clone(), details.clone()))));
            if let Err(e) = self.emit_all(evs).await {
                tracing::error!("spend alert: {e:#}");
            }
        }
    }
}
