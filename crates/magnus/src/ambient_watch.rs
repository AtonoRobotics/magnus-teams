//! Bounded Ambient Watch Loop for proactive autonomy (W3.3, M11, N13).
//!
//! - Incoming signals are triaged by a zero-tool Quarantined Triage Reader
//!   returning strictly typed fields (`actionable`, `urgency`, `category`).
//! - Enforces both hourly (`MAX_AMBIENT_RUNS_PER_HOUR = 10`) and daily
//!   (`MAX_AMBIENT_RUNS_PER_DAY = 50`) per-person caps so an external attacker
//!   cannot flood ambient triggers or drain spend.
//! - Ambient work stages Magnus-held drafts/cards only (never external native
//!   Gmail drafts that sync to user devices without review).

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

pub const MAX_AMBIENT_RUNS_PER_HOUR: usize = 10;
pub const MAX_AMBIENT_RUNS_PER_DAY: usize = 50;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    Ignore,
    Low,
    Medium,
    High,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SignalCategory {
    MeetingPrep,
    Blocker,
    CiFailure,
    Deadline,
    FollowUp,
    Other,
}

/// Strictly typed output of the zero-tool Quarantined Triage Reader (M11).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriageDecision {
    pub actionable: bool,
    pub urgency: Urgency,
    pub category: SignalCategory,
}

/// Per-person sliding-window rate limiter enforcing both hourly and daily caps.
#[derive(Default, Debug)]
pub struct AmbientQuota {
    firings: HashMap<String, VecDeque<DateTime<Utc>>>,
}

impl AmbientQuota {
    /// Returns `Ok(())` and records the firing if `person` is within both
    /// `MAX_AMBIENT_RUNS_PER_HOUR` and `MAX_AMBIENT_RUNS_PER_DAY`, else `Err(reason)`.
    pub fn try_consume(&mut self, person: &str, now: DateTime<Utc>) -> Result<(), String> {
        let q = self.firings.entry(person.to_string()).or_default();
        let day_cutoff = now - Duration::hours(24);
        while q.front().is_some_and(|&t| t < day_cutoff) {
            q.pop_front();
        }
        if q.len() >= MAX_AMBIENT_RUNS_PER_DAY {
            return Err(format!("daily ambient watch limit ({MAX_AMBIENT_RUNS_PER_DAY}/day) reached"));
        }
        let hour_cutoff = now - Duration::hours(1);
        let in_last_hour = q.iter().filter(|&&t| t >= hour_cutoff).count();
        if in_last_hour >= MAX_AMBIENT_RUNS_PER_HOUR {
            return Err(format!("hourly ambient watch limit ({MAX_AMBIENT_RUNS_PER_HOUR}/hour) reached"));
        }
        q.push_back(now);
        Ok(())
    }
}

/// Returns `true` iff a staging target is allowed for an ambient background run:
/// strictly Magnus-held drafts/cards only—never external native drafts (M11).
pub fn ambient_staging_allowed(is_magnus_held: bool) -> bool {
    is_magnus_held
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_hourly_and_daily_ambient_caps() {
        let mut quota = AmbientQuota::default();
        let start = Utc::now();
        for _ in 0..MAX_AMBIENT_RUNS_PER_HOUR {
            assert!(quota.try_consume("alice", start).is_ok());
        }
        assert!(quota.try_consume("alice", start).is_err(), "hourly cap must block 11th run");

        // Advance 2 hours each batch until daily cap (50) is hit.
        let mut t = start;
        for _ in 0..4 {
            t += Duration::hours(2);
            for _ in 0..MAX_AMBIENT_RUNS_PER_HOUR {
                assert!(quota.try_consume("alice", t).is_ok());
            }
        }
        t += Duration::hours(2);
        assert!(quota.try_consume("alice", t).is_err(), "daily cap (50/day) must block 51st run");
        assert!(ambient_staging_allowed(true));
        assert!(!ambient_staging_allowed(false));
    }
}
