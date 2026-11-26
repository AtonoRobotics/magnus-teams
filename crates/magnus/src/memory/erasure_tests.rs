//! Phase 4 live Postgres tests for atomic person erasure and concurrent fencing.

use crate::memory::enrich::{claim_next_job, commit_enrichment, EnrichCommit, EnrichmentOutput};
use crate::memory::erasure::{erase_owner, erase_owner_across_tenants, erase_person_if_present, ErasureStats};
use crate::memory::ingest::{ingest_source, IngestOutcome};
use crate::memory::live_pg_test::{LiveDb, TEST_EMBEDDING_DIM};
use crate::memory::{AssertionInput, AssertionKind, EdgeInput, EntityInput, SourceInput, TaintLabel};
use crate::memory_index::{MemoryConfig, MemoryMode};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn erase_owner_removes_all_person_data_cascades_supersessions_and_leaves_others_intact() {
    let db = LiveDb::create().await;
    let mut store = db.store().await;
    let tenant = format!("te_{}", Uuid::new_v4());
    let now = Utc::now();

    // Seed Bob's shared fact + Alice's private override + Alice's profile + Alice-only edge/entities.
    let s_bob = SourceInput {
        tenant_id: tenant.clone(),
        source_id: "bob_doc".into(),
        rev: 1,
        owner_id: "bob".into(),
        readers: vec![vec!["org:acme".into()]],
        label: TaintLabel { untrusted: false, private: false },
        text: "Acme lead time is 4 weeks.".into(),
        occurred_at: now,
    };
    let s_alice = SourceInput {
        tenant_id: tenant.clone(),
        source_id: "alice_mail".into(),
        rev: 1,
        owner_id: "alice".into(),
        readers: vec![vec!["user:alice".into()]],
        label: TaintLabel { untrusted: false, private: true },
        text: "Private update: Acme lead time is 6 weeks.".into(),
        occurred_at: now,
    };
    assert!(matches!(ingest_source(&mut store, &s_bob).await.unwrap(), IngestOutcome::Inserted { .. }));
    assert!(matches!(ingest_source(&mut store, &s_alice).await.unwrap(), IngestOutcome::Inserted { .. }));

    while let Some(job) = claim_next_job(&mut store).await.unwrap() {
        let out = if job.source_id == "bob_doc" {
            EnrichmentOutput {
                assertions: vec![AssertionInput {
                    assertion_id: "a_bob".into(),
                    kind: AssertionKind::Fact,
                    owner_id: "bob".into(),
                    statement: "Acme lead time is 4 weeks".into(),
                    valid_from: now,
                    valid_to: None,
                    confidence: 1.0,
                    embedding: None,
                }],
                edges: vec![EdgeInput {
                    edge_id: "e_bob".into(),
                    src: EntityInput { entity_type: "supplier".into(), display_name: "Acme".into(), external_id: Some("SUP-1".into()) },
                    dst: EntityInput { entity_type: "city".into(), display_name: "Austin".into(), external_id: None },
                    rel_type: "hq".into(),
                    valid_from: now,
                    valid_to: None,
                    properties: json!({}),
                }],
            }
        } else {
            EnrichmentOutput {
                assertions: vec![
                    AssertionInput {
                        assertion_id: "a_alice_override".into(),
                        kind: AssertionKind::Fact,
                        owner_id: "alice".into(),
                        statement: "Acme lead time is 6 weeks".into(),
                        valid_from: now,
                        valid_to: None,
                        confidence: 1.0,
                        embedding: None,
                    },
                    AssertionInput {
                        assertion_id: "a_alice_profile".into(),
                        kind: AssertionKind::Profile,
                        owner_id: "alice".into(),
                        statement: "Alice prefers bullet points".into(),
                        valid_from: now,
                        valid_to: None,
                        confidence: 1.0,
                        embedding: None,
                    },
                ],
                edges: vec![EdgeInput {
                    edge_id: "e_alice".into(),
                    src: EntityInput { entity_type: "supplier".into(), display_name: "Acme".into(), external_id: Some("SUP-1".into()) },
                    dst: EntityInput { entity_type: "contact".into(), display_name: "Alice Private Contact".into(), external_id: None },
                    rel_type: "contact".into(),
                    valid_from: now,
                    valid_to: None,
                    properties: json!({}),
                }],
            }
        };
        assert_eq!(commit_enrichment(&mut store, &job, &out).await.unwrap(), EnrichCommit::Committed);
    }
    store.record_supersession(&tenant, "a_alice_override", "a_bob").await.unwrap();

    // Claim an in-flight Alice job to verify erasure fences it out cleanly.
    let s_alice_inflight = SourceInput {
        tenant_id: tenant.clone(),
        source_id: "alice_inflight".into(),
        rev: 1,
        owner_id: "alice".into(),
        readers: vec![vec!["user:alice".into()]],
        label: TaintLabel { untrusted: false, private: true },
        text: "In-flight Alice note".into(),
        occurred_at: now,
    };
    assert!(matches!(ingest_source(&mut store, &s_alice_inflight).await.unwrap(), IngestOutcome::Inserted { .. }));
    let inflight_job = claim_next_job(&mut store).await.unwrap().expect("inflight_job");

    // Erase Alice atomically.
    let stats = erase_owner(&mut store, &tenant, "alice").await.unwrap();
    assert_eq!(stats.sources_deleted, 2);
    assert_eq!(stats.assertions_deleted, 2);
    assert_eq!(stats.edges_deleted, 1);
    assert_eq!(stats.entities_deleted, 1, "only Alice's private contact entity should be swept; SUP-1 stays for Bob");

    // In-flight enrichment commit must be fenced and roll back.
    let late_out = EnrichmentOutput {
        assertions: vec![AssertionInput {
            assertion_id: "a_late".into(),
            kind: AssertionKind::Fact,
            owner_id: "alice".into(),
            statement: "Should never land".into(),
            valid_from: now,
            valid_to: None,
            confidence: 1.0,
            embedding: None,
        }],
        edges: vec![],
    };
    assert_eq!(commit_enrichment(&mut store, &inflight_job, &late_out).await.unwrap(), EnrichCommit::FencedDeleted);

    // Zero rows for Alice; Bob's data and shared SUP-1 entity remain intact; supersession cascaded.
    let alice_left: i64 = store.client().query_one("SELECT count(*) FROM public.memory_assertions WHERE tenant_id = $1 AND owner_id = 'alice'", &[&tenant]).await.unwrap().get(0);
    let bob_left: i64 = store.client().query_one("SELECT count(*) FROM public.memory_assertions WHERE tenant_id = $1 AND assertion_id = 'a_bob'", &[&tenant]).await.unwrap().get(0);
    let sup_left: i64 = store.client().query_one("SELECT count(*) FROM public.memory_supersessions WHERE tenant_id = $1", &[&tenant]).await.unwrap().get(0);
    assert_eq!(alice_left, 0);
    assert_eq!(bob_left, 1);
    assert_eq!(sup_left, 0);

    // Cross-tenant erasure removes Bob across tenants.
    let bob_stats = erase_owner_across_tenants(&mut store, "bob").await.unwrap();
    assert_eq!(bob_stats.sources_deleted, 1);
    assert_eq!(bob_stats.assertions_deleted, 1);
    assert_eq!(bob_stats.edges_deleted, 1);
    assert_eq!(bob_stats.entities_deleted, 2);
}

/// Rolled back to Legacy after a Shadow/V2 period: the tables still hold the
/// person's rows, and erasing them must not depend on the configured mode.
#[tokio::test]
async fn erase_person_removes_unified_rows_after_rollback_to_legacy() {
    let db = LiveDb::create().await;
    let mut store = db.store().await;
    let src = SourceInput {
        tenant_id: "default".into(),
        source_id: "alice_note".into(),
        rev: 1,
        owner_id: "alice".into(),
        readers: vec![vec!["user:alice".into()]],
        label: TaintLabel { untrusted: false, private: true },
        text: "Alice's private note from the canary period.".into(),
        occurred_at: Utc::now(),
    };
    assert!(matches!(ingest_source(&mut store, &src).await.unwrap(), IngestOutcome::Inserted { .. }));

    let legacy = MemoryConfig { mode: MemoryMode::Legacy, embedding_dim: TEST_EMBEDDING_DIM, ..Default::default() };
    assert!(!legacy.is_unified_enabled());
    let stats = erase_person_if_present(&db.kernel_url, &legacy, "alice").await.unwrap();
    assert_eq!(stats.sources_deleted, 1);
    let left: i64 = store.client().query_one("SELECT count(*) FROM public.memory_sources WHERE owner_id = 'alice'", &[]).await.unwrap().get(0);
    assert_eq!(left, 0, "Legacy config must not leave the person's unified memory behind");
}

/// A database never migrated for unified memory has nothing to erase.
#[tokio::test]
async fn erase_person_is_a_no_op_without_unified_tables() {
    let db = LiveDb::create().await;
    let admin = crate::log::connect(&db.admin_url).await.unwrap();
    admin.batch_execute("DROP TABLE public.memory_sources CASCADE").await.unwrap();
    let legacy = MemoryConfig { mode: MemoryMode::Legacy, embedding_dim: TEST_EMBEDDING_DIM, ..Default::default() };
    assert_eq!(erase_person_if_present(&db.kernel_url, &legacy, "alice").await.unwrap(), ErasureStats::default());
}
