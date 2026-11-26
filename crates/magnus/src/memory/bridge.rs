//! Kernel bridge routing `MemoryMode::Shadow` and `MemoryMode::V2` searches
//! to the unified Postgres memory retriever and mapping results to `Remembered`.

use crate::memory::retrieve::{retrieve, HitOrigin, RetrieveRequest, RetrieveResponse};
use crate::memory::store::MemoryStore;
use crate::memory_index::MemoryConfig;
use crate::recall::Remembered;
use crate::state::MemoryEntry;
use crate::tools::Label;
use anyhow::Result;

/// Default tenant identifier for single-org appliance deployments.
pub const DEFAULT_TENANT_ID: &str = "default";
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

/// Execute unified Postgres retrieval for `person` and explicitly record access
/// counts for returned assertions.
pub async fn retrieve_for_person(kernel_url: &str, cfg: &MemoryConfig, person: &str, query: &str, allow_untrusted: bool) -> Result<Vec<Remembered>> {
    let mut store = MemoryStore::connect(kernel_url, cfg).await?;
    let mut req = RetrieveRequest::new(DEFAULT_TENANT_ID, person_principals(person), query);
    req.allow_untrusted = allow_untrusted;
    req.limit = MAX_MEMORY_HITS_RETURNED;

    let resp = retrieve(&mut store, &req).await?;
    let accessed: Vec<String> = resp.hits.iter().filter(|h| h.origin == HitOrigin::Assertion).map(|h| h.id.clone()).collect();
    if let Err(e) = store.record_access(DEFAULT_TENANT_ID, &accessed).await {
        tracing::warn!("memory::bridge record_access failed for {person}: {e:#}");
    }
    Ok(response_to_remembered(person, &resp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::enrich::{claim_next_job, commit_enrichment, EnrichCommit, EnrichmentOutput};
    use crate::memory::ingest::{ingest_source, IngestOutcome};
    use crate::memory::live_pg_test::{test_config, LiveDb};
    use crate::memory::{AssertionInput, AssertionKind, SourceInput, TaintLabel};
    use chrono::Utc;

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

        let access_count: i32 = store
            .client()
            .query_one("SELECT access_count FROM public.memory_assertions WHERE tenant_id = $1 AND assertion_id = 'a_bridge'", &[&DEFAULT_TENANT_ID])
            .await
            .unwrap()
            .get(0);
        assert_eq!(access_count, 1, "bridge must record_access for returned assertions");
    }
}
