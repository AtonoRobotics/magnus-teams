//! Kernel bridge serving every person's memory recall from the unified
//! Postgres store, and mapping its results to `Remembered`.

use crate::memory::retrieve::{recent_sources, retrieve, HitOrigin, RetrieveRequest, RetrieveResponse};
use crate::memory::store::MemoryStore;
use crate::memory_index::MemoryConfig;
use crate::recall::Remembered;
use crate::state::MemoryEntry;
use crate::tools::Label;
use anyhow::Result;

/// Maximum memory hits returned to `Kernel::memory_hits`.
pub const MAX_MEMORY_HITS_RETURNED: usize = 20;
/// Build the RLS principal set for a person's personal memory recall.
pub fn person_principals(person: &str) -> Vec<String> {
    vec![format!("user:{person}"), format!("person:{person}")]
}

/// Convert a `RetrieveResponse` into `Vec<Remembered>` for `Kernel::memory_hits`,
/// preserving per-hit taint and propagating `resp.label.private` (so private
/// supersession influence is never dropped by `memory_taint`).
pub fn response_to_remembered(person: &str, resp: &RetrieveResponse) -> Vec<Remembered> {
    resp.hits
        .iter()
        .take(MAX_MEMORY_HITS_RETURNED)
        .map(|hit| {
            let sources: Vec<String> = hit.citations.iter().map(|c| format!("{}@{}", c.source_id, c.rev)).collect();
            Remembered {
                entry: MemoryEntry {
                    id: hit.id.clone(),
                    person: person.to_string(),
                    run: String::new(),
                    label: Label { untrusted: hit.label.untrusted, private: hit.label.private || resp.label.private },
                    sources,
                    by: None,
                    at: hit.valid_from,
                    seq: 0,
                },
                fact: hit.text.clone(),
                topic: None,
            }
        })
        .collect()
}

/// Recall `person`'s memory matching `query` from the unified store, recording
/// access for the assertions returned. A query with no words to match ("what
/// do you remember?") lists their memories newest first.
pub async fn retrieve_for_person(kernel_url: &str, cfg: &MemoryConfig, person: &str, query: &str, allow_untrusted: bool) -> Result<Vec<Remembered>> {
    let mut store = MemoryStore::connect(kernel_url, cfg).await?;
    let mut req = RetrieveRequest::new(&cfg.tenant_id, person_principals(person), query);
    req.allow_untrusted = allow_untrusted;
    req.limit = MAX_MEMORY_HITS_RETURNED;

    let resp = if crate::memory::retrieve::has_terms(query) { retrieve(&mut store, &req).await? } else { recent_sources(&mut store, &req).await? };
    let accessed: Vec<String> = resp.hits.iter().filter(|h| h.origin == HitOrigin::Assertion).map(|h| h.id.clone()).collect();
    store.record_access(&cfg.tenant_id, &accessed).await?;
    Ok(response_to_remembered(person, &resp))
}

/// The memories `hits` cite, in rank order and each once, from hits whose
/// every cited source is one of the person's current memories (`current`, as
/// the state has them when the answer is served). A memory forgotten, trashed,
/// archived, superseded or erased is never served, whatever the store still
/// holds. Citations are `id@rev`.
pub fn cited_current(hits: &[Remembered], current: &std::collections::HashSet<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for h in hits {
        let ids: Vec<&str> = h.entry.sources.iter().map(|s| s.rsplit_once('@').map_or(s.as_str(), |(id, _)| id)).collect();
        if ids.is_empty() || !ids.iter().all(|id| current.contains(*id)) {
            continue;
        }
        for id in ids {
            if !out.iter().any(|o| o == id) {
                out.push(id.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::enrich::{claim_next_job, commit_enrichment, EnrichCommit, EnrichmentOutput};
    use crate::memory::ingest::{ingest_source, IngestOutcome};
    use crate::memory::live_pg_test::{test_config, LiveDb};
    use crate::memory::{AssertionInput, AssertionKind, SourceInput, TaintLabel};
    use crate::memory_index::DEFAULT_TENANT_ID;
    use chrono::Utc;

    #[test]
    fn only_memories_cited_by_wholly_current_hits_are_served() {
        let hit = |id: &str, sources: &[&str]| Remembered {
            entry: MemoryEntry {
                id: id.into(),
                person: "alice".into(),
                run: String::new(),
                label: Label::default(),
                sources: sources.iter().map(|s| s.to_string()).collect(),
                by: None,
                at: Utc::now(),
                seq: 0,
            },
            fact: "a fact".into(),
            topic: None,
        };
        let current: std::collections::HashSet<String> = ["mem_a".to_string(), "mem_b".to_string(), "mem_c".to_string()].into();
        let hits = [hit("x1", &["mem_a@3"]), hit("x2", &["mem_c@3", "mem_gone@4"]), hit("x3", &["mem_gone@4"]), hit("x4", &[]), hit("x5", &["mem_b@9", "mem_a@3"])];
        assert_eq!(cited_current(&hits, &current), vec!["mem_a", "mem_b"], "a hit citing any non-current memory, or none, is dropped; each memory once, in rank order");
    }

    #[tokio::test]
    async fn retrieval_uses_the_configured_tenant() {
        let db = LiveDb::create().await;
        let mut store = db.store().await;
        let now = Utc::now();
        let src = SourceInput {
            tenant_id: "acme".into(),
            source_id: "note_acme".into(),
            rev: 1,
            owner_id: "alice".into(),
            readers: vec![vec!["user:alice".into()]],
            label: TaintLabel { untrusted: false, private: false },
            text: "Alice books the quarterly offsite venue.".into(),
            occurred_at: now,
        };
        assert!(matches!(ingest_source(&mut store, &src).await.unwrap(), IngestOutcome::Inserted { .. }));
        let job = claim_next_job(&mut store).await.unwrap().expect("job");
        let out = EnrichmentOutput {
            assertions: vec![AssertionInput {
                assertion_id: "a_acme".into(),
                kind: AssertionKind::Fact,
                owner_id: "alice".into(),
                statement: "Alice books the quarterly offsite venue".into(),
                valid_from: now,
                valid_to: None,
                confidence: 1.0,
                embedding: None,
            }],
            edges: vec![],
        };
        assert_eq!(commit_enrichment(&mut store, &job, &out).await.unwrap(), EnrichCommit::Committed);

        let default_tenant = retrieve_for_person(&db.kernel_url, &test_config(), "alice", "offsite venue", true).await.unwrap();
        assert!(default_tenant.is_empty(), "another tenant's memory is not recalled");
        let acme = MemoryConfig { tenant_id: "acme".into(), ..test_config() };
        let hits = retrieve_for_person(&db.kernel_url, &acme, "alice", "offsite venue", true).await.unwrap();
        assert_eq!(hits.len(), 1);
    }

    async fn access_count(store: &MemoryStore, assertion_id: &str) -> i32 {
        store
            .client()
            .query_one("SELECT access_count FROM public.memory_assertions WHERE tenant_id = $1 AND assertion_id = $2", &[&DEFAULT_TENANT_ID, &assertion_id])
            .await
            .unwrap()
            .get(0)
    }

    #[tokio::test]
    async fn retrieve_for_person_returns_remembered_and_records_access() {
        let db = LiveDb::create().await;
        let mut store = db.store().await;
        let now = Utc::now();
        let src = SourceInput {
            tenant_id: DEFAULT_TENANT_ID.into(),
            source_id: "note_1".into(),
            rev: 1,
            owner_id: "alice".into(),
            readers: vec![vec!["user:alice".into()]],
            label: TaintLabel { untrusted: false, private: true },
            text: "Alice prefers weekly deployment summaries on Thursdays.".into(),
            occurred_at: now,
        };
        assert!(matches!(ingest_source(&mut store, &src).await.unwrap(), IngestOutcome::Inserted { .. }));
        let job = claim_next_job(&mut store).await.unwrap().expect("job");
        let out = EnrichmentOutput {
            assertions: vec![AssertionInput {
                assertion_id: "a_bridge".into(),
                kind: AssertionKind::Fact,
                owner_id: "alice".into(),
                statement: "Alice prefers weekly deployment summaries on Thursdays".into(),
                valid_from: now,
                valid_to: None,
                confidence: 1.0,
                embedding: None,
            }],
            edges: vec![],
        };
        assert_eq!(commit_enrichment(&mut store, &job, &out).await.unwrap(), EnrichCommit::Committed);

        let hits = retrieve_for_person(&db.kernel_url, &test_config(), "alice", "deployment summaries", true).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.id, "a_bridge");
        assert!(hits[0].entry.label.private);
        assert_eq!(hits[0].entry.sources, vec!["note_1@1".to_string()]);

        assert_eq!(access_count(&store, "a_bridge").await, 1, "bridge must record_access for returned assertions");
    }

    /// A query with no words lists the person's memories newest first, and
    /// only theirs.
    #[tokio::test]
    async fn a_query_with_no_words_lists_the_newest_memories() {
        let db = LiveDb::create().await;
        let mut store = db.store().await;
        let now = Utc::now();
        for (id, owner, text, age_min) in
            [("m_old", "alice", "Alice parks on level two.", 10), ("m_new", "alice", "Alice moved desks.", 1), ("m_bob", "bob", "Bob's locker is 14.", 0)]
        {
            let src = SourceInput {
                tenant_id: DEFAULT_TENANT_ID.into(),
                source_id: id.into(),
                rev: 1,
                owner_id: owner.into(),
                readers: vec![vec![format!("user:{owner}")]],
                label: TaintLabel { untrusted: false, private: true },
                text: text.into(),
                occurred_at: now - chrono::Duration::minutes(age_min),
            };
            assert!(matches!(ingest_source(&mut store, &src).await.unwrap(), IngestOutcome::Inserted { .. }));
        }
        let hits = retrieve_for_person(&db.kernel_url, &test_config(), "alice", "  ?  ", true).await.unwrap();
        let ids: Vec<&str> = hits.iter().map(|h| h.entry.sources[0].as_str()).collect();
        assert_eq!(ids, vec!["m_new@1", "m_old@1"], "alice's memories, newest first, and not bob's");
    }

    /// A word's stem finds its other forms, as recall did before the store.
    #[tokio::test]
    async fn a_word_finds_its_other_forms() {
        let db = LiveDb::create().await;
        let mut store = db.store().await;
        let src = SourceInput {
            tenant_id: DEFAULT_TENANT_ID.into(),
            source_id: "m_allergy".into(),
            rev: 1,
            owner_id: "alice".into(),
            readers: vec![vec!["user:alice".into()]],
            label: TaintLabel { untrusted: false, private: true },
            text: "Alice is allergic to peanuts; meetings are scheduled after ten.".into(),
            occurred_at: Utc::now(),
        };
        assert!(matches!(ingest_source(&mut store, &src).await.unwrap(), IngestOutcome::Inserted { .. }));
        for q in ["allergy", "schedules"] {
            let hits = retrieve_for_person(&db.kernel_url, &test_config(), "alice", q, true).await.unwrap();
            assert_eq!(hits.len(), 1, "{q} did not find its other form");
        }
    }
}
