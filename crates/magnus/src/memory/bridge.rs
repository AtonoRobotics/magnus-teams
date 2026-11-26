//! Kernel bridge routing `MemoryMode::Shadow` and `MemoryMode::V2` searches
//! to the unified Postgres memory retriever and mapping results to `Remembered`.

use crate::memory::retrieve::{retrieve, HitOrigin, RetrieveRequest, RetrieveResponse};
use crate::memory::store::MemoryStore;
use crate::memory_index::MemoryConfig;
use crate::recall::Remembered;
use crate::state::MemoryEntry;
use crate::tools::Label;
use anyhow::Result;
use std::time::{Duration, Instant};

/// Maximum memory hits returned to `Kernel::memory_hits`.
pub const MAX_MEMORY_HITS_RETURNED: usize = 20;
/// Longest a `MemoryMode::Shadow` comparison may run before it is abandoned.
/// It runs detached from the live request, so this bounds the background work
/// only; the person's answer never waits on it.
pub const SHADOW_RECALL_TIMEOUT_MS: Duration = Duration::from_millis(5_000);

/// Whether a retrieval counts as the person using the returned assertions.
/// Only recall that is served to the model raises `access_count`; a Shadow
/// comparison must leave the curator's ranking and cold-archival inputs alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessTracking {
    Record,
    Skip,
}

/// How a detached Shadow comparison ended.
#[derive(Debug)]
pub enum ShadowOutcome {
    Compared { v2_count: usize, v2_elapsed_ms: u64 },
    Failed(String),
    TimedOut,
}

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

/// Execute unified Postgres retrieval for `person`, recording access counts for
/// returned assertions only when `tracking` is `AccessTracking::Record`.
pub async fn retrieve_for_person(kernel_url: &str, cfg: &MemoryConfig, person: &str, query: &str, allow_untrusted: bool, tracking: AccessTracking) -> Result<Vec<Remembered>> {
    let mut store = MemoryStore::connect(kernel_url, cfg).await?;
    let mut req = RetrieveRequest::new(&cfg.tenant_id, person_principals(person), query);
    req.allow_untrusted = allow_untrusted;
    req.limit = MAX_MEMORY_HITS_RETURNED;

    let resp = retrieve(&mut store, &req).await?;
    if tracking == AccessTracking::Skip {
        return Ok(response_to_remembered(person, &resp));
    }
    let accessed: Vec<String> = resp.hits.iter().filter(|h| h.origin == HitOrigin::Assertion).map(|h| h.id.clone()).collect();
    if let Err(e) = store.record_access(&cfg.tenant_id, &accessed).await {
        tracing::warn!("memory::bridge record_access failed for {person}: {e:#}");
    }
    Ok(response_to_remembered(person, &resp))
}

/// Keep only hits whose every cited source is one of the person's current
/// memories (`current`, as the state has them when the answer is served). A
/// memory forgotten, trashed, archived or superseded after the in-step check
/// is never served, whatever the store still holds. Citations are `id@rev`.
pub fn only_current(hits: Vec<Remembered>, current: &std::collections::HashSet<String>) -> Vec<Remembered> {
    hits.into_iter().filter(|h| !h.entry.sources.is_empty() && h.entry.sources.iter().all(|s| current.contains(s.rsplit_once('@').map_or(s.as_str(), |(id, _)| id)))).collect()
}

/// What `MemoryMode::V2` serves: the v2 hits when there are any. No hits, or
/// an error, falls back to legacy (`None`). The unified store only holds what
/// has been ingested into it, so an empty answer must not hide the facts the
/// person has in legacy memory.
pub fn v2_hits_or_fallback(person: &str, result: Result<Vec<Remembered>>) -> Option<Vec<Remembered>> {
    match result {
        Ok(hits) if !hits.is_empty() => Some(hits),
        Ok(_) => {
            tracing::info!(person = %person, "memory v2 recall found nothing; falling back to legacy");
            None
        }
        Err(e) => {
            tracing::warn!(person = %person, "memory v2 recall failed; falling back to legacy: {e:#}");
            None
        }
    }
}

/// Run the `MemoryMode::Shadow` v2 retrieval detached from the live request,
/// bounded by `timeout`, read-only (`AccessTracking::Skip`), and log how it
/// compared with the `legacy_count` hits already returned. The caller does not
/// await the handle; it exists for tests.
pub fn spawn_shadow_comparison(
    kernel_url: String,
    cfg: MemoryConfig,
    person: String,
    query: String,
    legacy_count: usize,
    timeout: Duration,
) -> tokio::task::JoinHandle<ShadowOutcome> {
    tokio::spawn(async move {
        let t0 = Instant::now();
        let outcome = match tokio::time::timeout(timeout, retrieve_for_person(&kernel_url, &cfg, &person, &query, true, AccessTracking::Skip)).await {
            Ok(Ok(v2_hits)) => ShadowOutcome::Compared { v2_count: v2_hits.len(), v2_elapsed_ms: u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX) },
            Ok(Err(e)) => ShadowOutcome::Failed(format!("{e:#}")),
            Err(_) => ShadowOutcome::TimedOut,
        };
        match &outcome {
            ShadowOutcome::Compared { v2_count, v2_elapsed_ms } => {
                tracing::info!(person = %person, legacy_count, v2_count, v2_elapsed_ms, "memory shadow recall comparison")
            }
            ShadowOutcome::Failed(e) => tracing::warn!(person = %person, "memory shadow recall failed: {e}"),
            ShadowOutcome::TimedOut => tracing::warn!(person = %person, timeout_ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX), "memory shadow recall timed out"),
        }
        outcome
    })
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
    fn only_hits_citing_current_memories_are_served() {
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
        let current: std::collections::HashSet<String> = ["mem_a".to_string(), "mem_b".to_string()].into();
        let kept =
            only_current(vec![hit("x1", &["mem_a@3"]), hit("x2", &["mem_a@3", "mem_gone@4"]), hit("x3", &["mem_gone@4"]), hit("x4", &[]), hit("x5", &["mem_b@9"])], &current);
        let ids: Vec<&str> = kept.iter().map(|h| h.entry.id.as_str()).collect();
        assert_eq!(ids, vec!["x1", "x5"], "a hit citing any non-current memory, or none, is dropped");
    }

    #[test]
    fn v2_serves_its_hits_and_falls_back_to_legacy_when_empty_or_failed() {
        let hit = |id: &str| Remembered {
            entry: MemoryEntry { id: id.into(), person: "alice".into(), run: String::new(), label: Label::default(), sources: vec![], by: None, at: Utc::now(), seq: 0 },
            fact: "a fact".into(),
            topic: None,
        };
        let served = v2_hits_or_fallback("alice", Ok(vec![hit("a1")])).expect("v2 hits are served");
        assert_eq!(served[0].entry.id, "a1");
        assert!(v2_hits_or_fallback("alice", Ok(vec![])).is_none(), "an empty v2 answer falls back to legacy");
        assert!(v2_hits_or_fallback("alice", Err(anyhow::anyhow!("db down"))).is_none());
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

        let default_tenant = retrieve_for_person(&db.kernel_url, &test_config(), "alice", "offsite venue", true, AccessTracking::Skip).await.unwrap();
        assert!(default_tenant.is_empty(), "another tenant's memory is not recalled");
        let acme = MemoryConfig { tenant_id: "acme".into(), ..test_config() };
        let hits = retrieve_for_person(&db.kernel_url, &acme, "alice", "offsite venue", true, AccessTracking::Skip).await.unwrap();
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

        // A Shadow comparison reads the same hits but is not the person using them.
        let shadow = retrieve_for_person(&db.kernel_url, &test_config(), "alice", "deployment summaries", true, AccessTracking::Skip).await.unwrap();
        assert_eq!(shadow.len(), 1);
        assert_eq!(access_count(&store, "a_bridge").await, 0, "a Skip retrieval must not raise access_count");

        let hits = retrieve_for_person(&db.kernel_url, &test_config(), "alice", "deployment summaries", true, AccessTracking::Record).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.id, "a_bridge");
        assert!(hits[0].entry.label.private);
        assert_eq!(hits[0].entry.sources, vec!["note_1@1".to_string()]);

        assert_eq!(access_count(&store, "a_bridge").await, 1, "bridge must record_access for returned assertions");
    }

    #[tokio::test]
    async fn shadow_comparison_is_read_only() {
        let db = LiveDb::create().await;
        let mut store = db.store().await;
        let now = Utc::now();
        let src = SourceInput {
            tenant_id: DEFAULT_TENANT_ID.into(),
            source_id: "note_shadow".into(),
            rev: 1,
            owner_id: "alice".into(),
            readers: vec![vec!["user:alice".into()]],
            label: TaintLabel { untrusted: false, private: false },
            text: "Alice reviews vendor invoices every Monday.".into(),
            occurred_at: now,
        };
        assert!(matches!(ingest_source(&mut store, &src).await.unwrap(), IngestOutcome::Inserted { .. }));
        let job = claim_next_job(&mut store).await.unwrap().expect("job");
        let out = EnrichmentOutput {
            assertions: vec![AssertionInput {
                assertion_id: "a_shadow".into(),
                kind: AssertionKind::Fact,
                owner_id: "alice".into(),
                statement: "Alice reviews vendor invoices every Monday".into(),
                valid_from: now,
                valid_to: None,
                confidence: 1.0,
                embedding: None,
            }],
            edges: vec![],
        };
        assert_eq!(commit_enrichment(&mut store, &job, &out).await.unwrap(), EnrichCommit::Committed);

        let outcome = spawn_shadow_comparison(db.kernel_url.clone(), test_config(), "alice".into(), "vendor invoices".into(), 0, SHADOW_RECALL_TIMEOUT_MS).await.unwrap();
        assert!(matches!(outcome, ShadowOutcome::Compared { v2_count: 1, .. }), "{outcome:?}");
        assert_eq!(access_count(&store, "a_shadow").await, 0, "Shadow must not raise access_count");
    }

    /// A database that accepts the connection and never answers: the Shadow
    /// comparison must neither hold the caller nor run past its timeout.
    #[tokio::test]
    async fn shadow_comparison_does_not_block_the_caller_and_times_out() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let _hold = tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((sock, _)) = listener.accept().await {
                held.push(sock);
            }
        });
        let url = format!("postgres://k:pw@127.0.0.1:{port}/db");
        let timeout = std::time::Duration::from_millis(300);

        let started = std::time::Instant::now();
        let handle = spawn_shadow_comparison(url, test_config(), "alice".into(), "anything".into(), 3, timeout);
        assert!(started.elapsed() < std::time::Duration::from_millis(50), "spawning must not wait on the database");

        let outcome = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await.expect("shadow comparison must honour its timeout").unwrap();
        assert!(matches!(outcome, ShadowOutcome::TimedOut), "{outcome:?}");
    }
}
