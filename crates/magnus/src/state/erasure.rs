//! Each person erased with content, by their id then: kept for good, as
//! the record that it happened (#195), with whether a backup still holds
//! what was removed.

use super::State;
use crate::forget::Erased;
use crate::log::Event;
use std::collections::BTreeMap;

/// A person erased with content: the removal that took it, their provider
/// subject then (the console's name for them), and whether a backup still
/// holds it.
#[derive(Clone, Debug)]
pub struct Erasure {
    pub removal: i64,
    pub subject: Option<String>,
    pub pending: bool,
}

#[derive(Default, Debug)]
pub struct Erasures {
    by_person: BTreeMap<String, Erasure>,
}

impl crate::forget::Forget for Erasures {
    /// Kept: the record of the erasure itself (#195).
    fn forget(&mut self, _: &Erased) {}
}

impl Erasures {
    /// A person's content removed by the event at `removal`, under the
    /// provider subject they were bound to then.
    pub(super) fn record(&mut self, person: &str, removal: i64, subject: Option<String>) {
        self.by_person.insert(person.to_string(), Erasure { removal, subject, pending: false });
    }

    /// Whether this id was a person erased here.
    pub fn contains(&self, person: &str) -> bool {
        self.by_person.contains_key(person)
    }

    /// How many erasures a backup still holds.
    pub fn pending(&self) -> usize {
        self.by_person.values().filter(|e| e.pending).count()
    }

    #[cfg(test)]
    pub(crate) fn canonical_fields(&self) -> Vec<(&'static str, String)> {
        vec![("erasures", format!("{:?}", self.by_person))]
    }
}

impl State {
    /// Which erasures a backup still holds, after a scrub's record: each
    /// change is in the audit, as the console names the person (#195).
    pub(super) fn erasures_pending(&mut self, e: &Event) {
        let pending = self.server_ops.scrub_pending().cloned();
        for (person, er) in self.erasures.by_person.iter_mut() {
            let now = pending.as_ref().is_some_and(|p| p.from < er.removal && er.removal <= p.through);
            if now == er.pending {
                continue;
            }
            er.pending = now;
            let (action, after) = match &pending {
                Some(p) if now => ("user_erasure_incomplete", serde_json::json!({ "backups": p.files, "detail": p.detail })),
                _ => ("user_erasure_completed", serde_json::json!({ "backups": "every backup kept is rewritten without their content" })),
            };
            let target = er.subject.clone().unwrap_or_else(|| person.clone());
            self.org.note_audit(e, serde_json::json!({ "kind": "admin_action", "admin": "kernel", "name": "Magnus", "action": action, "target": target, "source": null, "before": null, "after": after }));
        }
    }
}
