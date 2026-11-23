//! Persisted security state folded from the event log (W1.4, N6).
//!
//! Keeps turn/firing-scoped `query_gate` budgets and every `safety::BLOCKED` hash
//! across server restarts without volatile `static Mutex` resets.

use crate::compartment::query_gate::QueryBudget;
use crate::events::{QueryGatePayload, SafetyBlockedPayload};
use std::collections::{HashMap, VecDeque};

/// Maximum tracked scopes/entries retained in memory before oldest-first pruning.
const MAX_PERSISTED_SECURITY_ENTRIES: usize = 4096;
/// The most blocked sends kept for one run and tool, oldest dropped first.
const MAX_BLOCKED_SENDS_PER_TOOL: usize = 64;

/// The sends blocked for one run and tool: every blocked message's hash,
/// with why, oldest first. A message blocked once stays blocked, however
/// many others were blocked after it (2026-11-23).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Blocked(Vec<(String, String)>);

impl Blocked {
    /// One more blocked message; one blocked before keeps its place and takes the newer reason.
    pub fn add(&mut self, hash: &str, reason: &str) {
        self.0.retain(|(h, _)| h != hash);
        if self.0.len() >= MAX_BLOCKED_SENDS_PER_TOOL {
            self.0.remove(0);
        }
        self.0.push((hash.to_string(), reason.to_string()));
    }

    /// Every block in `other`, after these.
    pub fn merge(&mut self, other: &Blocked) {
        for (h, r) in &other.0 {
            self.add(h, r);
        }
    }

    /// Why this very message was blocked, if it was.
    pub fn reason_for(&self, hash: &str) -> Option<&str> {
        self.0.iter().find(|(h, _)| h == hash).map(|(_, r)| r.as_str())
    }

    /// Why the latest block was made.
    pub fn last_reason(&self) -> Option<&str> {
        self.0.last().map(|(_, r)| r.as_str())
    }
}

#[derive(Clone, Debug, Default)]
pub struct SecurityLedger {
    query_budgets: HashMap<String, QueryBudget>,
    query_order: VecDeque<String>,
    safety_blocked: HashMap<String, Blocked>,
    blocked_order: VecDeque<String>,
}

impl SecurityLedger {
    pub fn apply_query_gate(&mut self, p: &QueryGatePayload) {
        if !self.query_budgets.contains_key(&p.scope) {
            if self.query_order.len() >= MAX_PERSISTED_SECURITY_ENTRIES {
                if let Some(old) = self.query_order.pop_front() {
                    self.query_budgets.remove(&old);
                }
            }
            self.query_order.push_back(p.scope.clone());
        }
        let entry = self.query_budgets.entry(p.scope.clone()).or_default();
        entry.bytes_used = entry.bytes_used.saturating_add(p.bytes_added);
        if p.denied {
            entry.denials = entry.denials.saturating_add(1);
        }
    }

    pub fn apply_safety_blocked(&mut self, p: &SafetyBlockedPayload) {
        let key = format!("{}\n{}", p.run, p.tool);
        if !self.safety_blocked.contains_key(&key) {
            if self.blocked_order.len() >= MAX_PERSISTED_SECURITY_ENTRIES {
                if let Some(old) = self.blocked_order.pop_front() {
                    self.safety_blocked.remove(&old);
                }
            }
            self.blocked_order.push_back(key.clone());
        }
        self.safety_blocked.entry(key).or_default().add(&p.hash, &p.reason);
    }

    pub fn query_budget(&self, scope: &str) -> QueryBudget {
        self.query_budgets.get(scope).copied().unwrap_or_default()
    }

    pub fn blocked(&self, run: &str, tool: &str) -> Option<Blocked> {
        let key = format!("{run}\n{tool}");
        self.safety_blocked.get(&key).cloned()
    }
}

impl crate::forget::Forget for SecurityLedger {
    fn forget(&mut self, who: &crate::forget::Erased) {
        self.query_budgets.retain(|scope, _| !who.tasks.contains(scope) && !who.runs.contains(scope));
        self.query_order.retain(|scope| self.query_budgets.contains_key(scope));
        self.safety_blocked.retain(|k, _| k.split_once('\n').is_none_or(|(r, _)| !who.runs.contains(r)));
        self.blocked_order.retain(|k| self.safety_blocked.contains_key(k));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_budgets_and_blocked_survive_replay() {
        let mut ledger = SecurityLedger::default();
        ledger.apply_query_gate(&QueryGatePayload { scope: "turn_1".into(), bytes_added: 1024, denied: false });
        ledger.apply_query_gate(&QueryGatePayload { scope: "turn_1".into(), bytes_added: 0, denied: true });
        ledger.apply_safety_blocked(&SafetyBlockedPayload { run: "r1".into(), tool: "gmail__send".into(), hash: "abc123".into(), reason: "outside envelope".into() });

        let b = ledger.query_budget("turn_1");
        assert_eq!(b.bytes_used, 1024);
        assert_eq!(b.denials, 1);
        let blocked = ledger.blocked("r1", "gmail__send").unwrap();
        assert_eq!(blocked.reason_for("abc123"), Some("outside envelope"));
    }
}
