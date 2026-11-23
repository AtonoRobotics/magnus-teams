//! Bounded Ambient Watch Loop for proactive autonomy (W3.3, M11, N13).
//!
//! - Incoming signals are triaged by a zero-tool Quarantined Triage Reader
//!   returning strictly typed fields (`actionable`, `urgency`, `category`).
//! - The per-person caps on background wakes (10 an hour, 50 a day) are
//!   `wake_budget`'s, checked where every review starts.
//! - Ambient work stages Magnus-held drafts/cards only (never external native
//!   Gmail drafts that sync to user devices without review).

use serde::{Deserialize, Serialize};

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

/// Returns `true` iff a staging target is allowed for an ambient background run:
/// strictly Magnus-held drafts/cards only—never external native drafts (M11).
pub fn ambient_staging_allowed(is_magnus_held: bool) -> bool {
    is_magnus_held
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambient_staging_is_magnus_held_only() {
        assert!(ambient_staging_allowed(true));
        assert!(!ambient_staging_allowed(false));
    }
}
