//! Persisted security state folded from the event log (W1.4, N6).
//!
//! Keeps turn/firing-scoped `query_gate` budgets and `safety::BLOCKED` hashes
//! across server restarts without volatile `static Mutex` resets.

use crate::compartment::query_gate::QueryBudget;
use crate::events::{QueryGatePayload, SafetyBlockedPayload};
use std::collections::{HashMap, VecDeque};

/// Maximum tracked scopes/entries retained in memory before oldest-first pruning.
const MAX_PERSISTED_SECURITY_ENTRIES: usize = 4096;

#[derive(Clone, Debug, Default)]
pub struct SecurityLedger {
    query_budgets: HashMap<String, QueryBudget>,
    query_order: VecDeque<String>,
    safety_blocked: HashMap<String, (String, String)>,
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
        self.safety_blocked.insert(key, (p.hash.clone(), p.reason.clone()));
    }

    pub fn query_budget(&self, scope: &str) -> QueryBudget {
        self.query_budgets.get(scope).copied().unwrap_or_default()
    }

    pub fn blocked(&self, run: &str, tool: &str) -> Option<(String, String)> {
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
        assert_eq!(ledger.blocked("r1", "gmail__send"), Some(("abc123".into(), "outside envelope".into())));
    }
}
