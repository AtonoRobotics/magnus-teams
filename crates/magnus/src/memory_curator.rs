//! Label-safe bi-temporal memory reconciliation and Sleep-Time Curator (W3.1, N1, P4, O2).
//!
//! - Anti-poisoning-by-deletion (`N1`, `P4`): compares ONLY the `untrusted` bit
//!   (`!run_label.untrusted || old_label.untrusted`). A web-tainted run can never
//!   supersede a clean fact, while a clean private run (e.g. after reading mail)
//!   can still supersede a clean non-private fact; the new fact keeps its own `Label`.
//! - Sleep-Time Curator never merges across different `untrusted` bits, and
//!   archives Tier-2 untrusted facts past `TIER2_COLD_DAYS` via `Ev::MemoryArchived`
//!   instead of silently deleting user data (`O2`).

use crate::events::{Ev, MemoryArchivedPayload, MemorySupersededPayload};
use crate::kernel::{new_id, Kernel};
use crate::memory_index::TIER2_COLD_DAYS;
use crate::state::Run;
use crate::tools::Label;
use anyhow::{bail, Result};
use chrono::{DateTime, Duration, Utc};

/// Returns `true` iff a run with `run_label` is permitted to supersede an
/// existing fact with `old_label` (N1, P4: untrusted bit only).
pub fn may_supersede(run_label: Label, old_label: Label) -> bool {
    !run_label.untrusted || old_label.untrusted
}

/// Returns `true` iff two facts may be merged by the Sleep-Time Curator
/// (never merge across different `untrusted` bits; merged fact carries `a.join(b)`).
pub fn may_merge(a: Label, b: Label) -> bool {
    a.untrusted == b.untrusted
}

/// Returns `true` if a Tier-2 untrusted fact is old enough to be moved to cold
/// archive (`O2` — archived/down-ranked, never silently deleted).
pub fn should_archive_tier2(label: Label, created_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    label.untrusted && now.signed_duration_since(created_at) >= Duration::days(TIER2_COLD_DAYS)
}

impl Kernel {
    /// Supersede `old_id` with `new_fact`, enforcing `!run.untrusted || old.untrusted`
    /// so tainted runs can never poison-by-deletion a clean user fact (N1, P4).
    pub async fn supersede_memory(&self, run: &Run, old_id: &str, new_fact: String, topic: Option<String>, reason: String) -> Result<String> {
        let old = self
            .read(|s| s.library.memory().iter().find(|m| m.id == old_id && m.person == run.person).cloned())
            .ok_or_else(|| anyhow::anyhow!("memory {old_id} not found for person"))?;
        let run_label = Label { untrusted: run.untrusted, private: run.private };
        if !may_supersede(run_label, old.label) {
            bail!("refused: an untrusted run cannot supersede a clean user memory ({old_id})");
        }
        let new_id = new_id("mem");
        let sources: Vec<String> = run.private_from.iter().cloned().collect();
        self.emit_all(vec![
            (
                "kernel",
                Ev::Memory(crate::events::MemoryPayload {
                    id: new_id.clone(),
                    person: run.person.clone(),
                    run: run.id.clone(),
                    fact: new_fact,
                    topic,
                    label: run_label,
                    sources,
                    by: Some("reconciler".into()),
                }),
            ),
            (
                "kernel",
                Ev::MemorySuperseded(MemorySupersededPayload { old_id: old_id.into(), new_id: new_id.clone(), person: run.person.clone(), reason, valid_until: Utc::now() }),
            ),
        ])
        .await?;
        Ok(new_id)
    }

    /// Sleep-Time Curator pass: move Tier-2 untrusted facts older than
    /// `TIER2_COLD_DAYS` into cold archive (`Ev::MemoryArchived`, O2).
    pub async fn archive_cold_tier2(&self, person: &str, now: DateTime<Utc>) -> Result<usize> {
        let to_archive: Vec<String> = self.read(|s| {
            s.library
                .memory()
                .iter()
                .filter(|m| m.person == person && !s.memory_archived.contains(&m.id) && !s.memory_superseded.contains_key(&m.id))
                .filter(|m| should_archive_tier2(m.label, m.at, now))
                .map(|m| m.id.clone())
                .collect()
        });
        let mut count = 0usize;
        for id in to_archive {
            self.emit("curator", Ev::MemoryArchived(MemoryArchivedPayload { id, person: person.into(), reason: format!("tier2_cold_after_{TIER2_COLD_DAYS}_days") })).await?;
            count += 1;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tainted_run_cannot_supersede_clean_fact_but_clean_private_run_can() {
        let clean = Label { untrusted: false, private: false };
        let clean_private = Label { untrusted: false, private: true };
        let tainted = Label { untrusted: true, private: false };
        let both = Label { untrusted: true, private: true };

        assert!(!may_supersede(tainted, clean), "web-tainted run must not supersede clean fact (N1)");
        assert!(!may_supersede(both, clean), "tainted+private run must not supersede clean fact (N1)");
        assert!(may_supersede(clean_private, clean), "clean private run must be allowed to supersede clean non-private fact (P4)");
        assert!(may_supersede(clean, clean_private));
        assert!(may_supersede(tainted, tainted));
        assert!(may_supersede(clean, tainted));
    }

    #[test]
    fn curator_never_merges_across_untrusted_bit() {
        let clean = Label { untrusted: false, private: false };
        let clean_private = Label { untrusted: false, private: true };
        let tainted = Label { untrusted: true, private: false };
        assert!(may_merge(clean, clean_private));
        assert!(!may_merge(clean, tainted));
        assert!(!may_merge(clean_private, tainted));
    }

    #[test]
    fn tier2_cold_archival_triggers_only_on_old_untrusted_facts() {
        let now = Utc::now();
        let old = now - Duration::days(TIER2_COLD_DAYS + 1);
        let fresh = now - Duration::days(5);
        assert!(should_archive_tier2(Label { untrusted: true, private: false }, old, now));
        assert!(!should_archive_tier2(Label { untrusted: true, private: false }, fresh, now));
        assert!(!should_archive_tier2(Label { untrusted: false, private: true }, old, now));
    }
}
