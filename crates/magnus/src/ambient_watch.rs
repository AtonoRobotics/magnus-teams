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

/// Fast, deterministic pre-triage for watched connector diffs so empty or
/// automated-noise polls do not burn an LLM review wake.
pub fn triage_watched_diff(app: &str, remote: &str, fresh_count: usize, data: &serde_json::Value) -> TriageDecision {
    if fresh_count == 0 {
        return TriageDecision { actionable: false, urgency: Urgency::Ignore, category: SignalCategory::Other };
    }
    let hay = format!("{app} {remote} {}", data.to_string().chars().take(1200).collect::<String>()).to_ascii_lowercase();
    if hay.contains("ci") && (hay.contains("fail") || hay.contains("error") || hay.contains("broken")) {
        return TriageDecision { actionable: true, urgency: Urgency::High, category: SignalCategory::CiFailure };
    }
    if hay.contains("urgent") || hay.contains("blocker") || hay.contains("incident") || hay.contains("p0") || hay.contains("outage") {
        return TriageDecision { actionable: true, urgency: Urgency::High, category: SignalCategory::Blocker };
    }
    if hay.contains("calendar") || hay.contains("meeting") || hay.contains("invite") {
        return TriageDecision { actionable: true, urgency: Urgency::Medium, category: SignalCategory::MeetingPrep };
    }
    if hay.contains("deadline") || hay.contains("overdue") || hay.contains("due ") {
        return TriageDecision { actionable: true, urgency: Urgency::Medium, category: SignalCategory::Deadline };
    }
    // Suppress automated newsletter / no-reply / unsubscribe-only noise when it is the sole signal.
    let noise = ["noreply@", "no-reply@", "newsletter", "unsubscribe", "marketing@", "notifications@github.com"];
    if fresh_count == 1 && noise.iter().any(|n| hay.contains(n)) && !hay.contains("review requested") && !hay.contains("assigned") {
        return TriageDecision { actionable: false, urgency: Urgency::Ignore, category: SignalCategory::Other };
    }
    TriageDecision { actionable: true, urgency: Urgency::Medium, category: SignalCategory::FollowUp }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ambient_staging_is_magnus_held_only() {
        assert!(ambient_staging_allowed(true));
        assert!(!ambient_staging_allowed(false));
    }

    #[test]
    fn triage_suppresses_noreply_noise_and_escalates_urgent_signals() {
        let noisy = triage_watched_diff("google-gmail", "list_messages", 1, &json!({ "from": "newsletter@shop.example", "body": "Click to unsubscribe" }));
        assert!(!noisy.actionable && noisy.urgency == Urgency::Ignore);
        let ci = triage_watched_diff("github", "list_runs", 1, &json!({ "name": "CI build", "conclusion": "failure" }));
        assert!(ci.actionable && ci.urgency == Urgency::High && ci.category == SignalCategory::CiFailure);
        let cal = triage_watched_diff("google-calendar", "list_events", 1, &json!({ "summary": "Board meeting" }));
        assert!(cal.actionable && cal.category == SignalCategory::MeetingPrep);
    }
}
