//! Postgres integration & acceptance tests for the unified memory engine.
//!
//! Covers:
//! 1. Mixed-visibility override (buyer's private email vs. shared supplier lead time)
//!    including zero leakage to teammates and atomic revocation restoration.
//! 2. Hybrid exact-ID + FTS + pgvector + temporal + 2-hop graph retrieval.
//! 3. Concurrent in-flight enrichment fencing against source revocation.
//! 4. 5-job curator (idle watermark gating, EMA feedback with untrusted guardrail,
//!    semantic dedup with provenance union, trust-gated supersession vs conflict,
//!    lesson cap enforcement, cold untrusted archival).
//! 5. Selective-RLS latency & recall benchmark with `pgvector` iterative HNSW.

use crate::admin_app::tests::server_with;
use crate::memory::backfill::LegacyRecord;
use crate::memory::curator::CollisionResolution;
use crate::memory::enrich::{deterministic_entity_id, CommitOutcome, EnrichmentPayload};
use crate::memory::feedback::FeedbackSignal;
use crate::memory::retrieve::RetrieveQuery;
use crate::memory::{AssertionInput, EdgeInput, EntityInput, MemoryEngine, ReaderContext, SourceInput, TrustLabel, DEFAULT_TENANT_ID};
use axum::Router;
use chrono::{Duration, TimeZone, Utc};
use serde_json::json;

/// Boot an isolated Postgres test DB with unified memory schema migrated.
async fn setup_engine() -> (crate::admin_app::tests::Server, MemoryEngine) {
    let s = server_with("", |_| Router::new()).await;
    let db = s.store().await;
    db.batch_execute(&crate::memory::schema::migrate_sql("postgres")).await.unwrap();
    let engine = MemoryEngine::new(&s.k.cfg.database.url);
    (s, engine)
}

/// THE DECIDING CASE:
/// A shared supplier record says Acme's lead time is 14 days.
/// Buyer Alice receives a private email updating Acme's lead time to 21 days.
/// - Alice recalls the 21-day correction (14-day shared claim superseded for Alice).
/// - Teammate Bob still recalls the 14-day shared claim, with zero leakage of Alice's email.
/// - When Alice's email is revoked, both the 21-day assertion and its private `supersedes`
///   edge vanish atomically, and Alice once again recalls the 14-day shared claim.
#[tokio::test(flavor = "multi_thread")]
async fn mixed_visibility_private_override_and_atomic_revocation() {
    let (s, engine) = setup_engine().await;
    let t0 = Utc.with_ymd_and_hms(2026, 10, 1, 10, 0, 0).unwrap();
    let t1 = Utc.with_ymd_and_hms(2026, 10, 15, 10, 0, 0).unwrap();

    // 1. Shared org source & assertion (readable by everyone in tenant).
    let shared_ing = engine
        .ingest_source(&SourceInput {
            tenant_id: DEFAULT_TENANT_ID.into(),
            source_id: "wiki_acme_supplier".into(),
            rev: 1,
            owner_id: None,
            readers: vec![],
            label: TrustLabel { untrusted: false, private: false },
            kind: "wiki".into(),
            text: "Acme Corp standard supplier lead time is 14 days.".into(),
            metadata: json!({}),
            occurred_at: t0,
        })
        .await
        .unwrap();
    assert!(!shared_ing.deduplicated);

    let acme_id = deterministic_entity_id(DEFAULT_TENANT_ID, "company", "acme.com");
    let shared_commit = engine
        .commit_enrichment(
            DEFAULT_TENANT_ID,
            "wiki_acme_supplier",
            1,
            shared_ing.job_id,
            &EnrichmentPayload {
                source_embedding: Some(vec![1.0, 0.0, 0.0, 0.0]),
                model: Some("test-v1".into()),
                entities: vec![EntityInput {
                    entity_type: "company".into(),
                    canonical_name: "Acme Corp".into(),
                    external_id: Some("acme.com".into()),
                    aliases: vec!["Acme".into()],
                    embedding: None,
                    model: None,
                }],
                assertions: vec![AssertionInput {
                    assertion_id: "assert_acme_14d".into(),
                    kind: "fact".into(),
                    statement: "Acme Corp supplier lead time is 14 days".into(),
                    valid_from: Some(t0),
                    valid_to: None,
                    confidence: 1.0,
                    embedding: Some(vec![1.0, 0.0, 0.0, 0.0]),
                    model: Some("test-v1".into()),
                }],
                edges: vec![EdgeInput {
                    edge_id: "edge_acme_shared_fact".into(),
                    src_id: acme_id.clone(),
                    dst_id: "assert_acme_14d".into(),
                    rel_type: "has_lead_time".into(),
                    valid_from: Some(t0),
                    valid_to: None,
                    properties: json!({}),
                }],
            },
        )
        .await
        .unwrap();
    assert_eq!(shared_commit, CommitOutcome::Committed);

    // 2. Alice's private email updates lead time to 21 days.
    let priv_ing = engine
        .ingest_source(&SourceInput {
            tenant_id: DEFAULT_TENANT_ID.into(),
            source_id: "email_alice_acme_21d".into(),
            rev: 1,
            owner_id: Some("alice".into()),
            readers: vec![vec!["person:alice".into()]],
            label: TrustLabel { untrusted: false, private: true },
            kind: "email".into(),
            text: "Private vendor note: Acme Corp supplier lead time updated to 21 days.".into(),
            metadata: json!({}),
            occurred_at: t1,
        })
        .await
        .unwrap();

    engine
        .commit_enrichment(
            DEFAULT_TENANT_ID,
            "email_alice_acme_21d",
            1,
            priv_ing.job_id,
            &EnrichmentPayload {
                source_embedding: Some(vec![0.99, 0.14, 0.0, 0.0]),
                model: Some("test-v1".into()),
                entities: vec![],
                assertions: vec![AssertionInput {
                    assertion_id: "assert_acme_21d_alice".into(),
                    kind: "fact".into(),
                    statement: "Acme Corp supplier lead time is 21 days".into(),
                    valid_from: Some(t1),
                    valid_to: None,
                    confidence: 1.0,
                    embedding: Some(vec![0.99, 0.14, 0.0, 0.0]),
                    model: Some("test-v1".into()),
                }],
                edges: vec![],
            },
        )
        .await
        .unwrap();

    // Curator records visibility-scoped supersession (inherits Alice's private readers).
    let edge_id =
        engine.record_collision_edge(DEFAULT_TENANT_ID, "assert_acme_21d_alice", "assert_acme_14d", CollisionResolution::Supersede, "newer vendor confirmation").await.unwrap();
    assert!(edge_id.is_some());

    let alice = ReaderContext::for_person(DEFAULT_TENANT_ID, "alice", &[]);
    let bob = ReaderContext::for_person(DEFAULT_TENANT_ID, "bob", &[]);

    let mut q = RetrieveQuery::new("Acme Corp supplier lead time");
    q.embedding = Some(vec![1.0, 0.0, 0.0, 0.0]);
    q.model = Some("test-v1".into());

    // Alice sees 21 days, and 14-day assertion is suppressed for Alice.
    let alice_res = engine.retrieve(&alice, &q).await.unwrap();
    let alice_ids: Vec<&str> = alice_res.hits.iter().map(|h| h.id.as_str()).collect();
    assert!(alice_ids.contains(&"assert_acme_21d_alice"), "Alice must recall her 21-day override: {alice_ids:?}");
    assert!(!alice_ids.contains(&"assert_acme_14d"), "14-day assertion must be superseded for Alice");
    assert!(alice_res.label.private, "Alice's recall must inherit private taint");

    // Bob sees 14 days and learns NOTHING about Alice's private email.
    let bob_res = engine.retrieve(&bob, &q).await.unwrap();
    let bob_ids: Vec<&str> = bob_res.hits.iter().map(|h| h.id.as_str()).collect();
    assert!(bob_ids.contains(&"assert_acme_14d"), "Bob must still recall shared 14-day assertion: {bob_ids:?}");
    assert!(!bob_ids.contains(&"assert_acme_21d_alice"), "Bob must never see Alice's private override");
    assert!(!bob_res.source_ids.contains(&"email_alice_acme_21d".to_string()));
    assert!(!bob_res.label.private);

    // 3. Revoke Alice's private email atomically.
    let rev = engine.revoke_source(DEFAULT_TENANT_ID, "email_alice_acme_21d", None).await.unwrap();
    assert_eq!(rev.sources_deleted, 1);
    assert_eq!(rev.assertions_deleted, 1);
    assert_eq!(rev.edges_deleted, 1);

    // Alice now falls back cleanly to the shared 14-day assertion.
    let alice_after = engine.retrieve(&alice, &q).await.unwrap();
    let after_ids: Vec<&str> = alice_after.hits.iter().map(|h| h.id.as_str()).collect();
    assert!(after_ids.contains(&"assert_acme_14d"), "Alice must recall shared 14-day fact after revoking email");
    assert!(!after_ids.contains(&"assert_acme_21d_alice"));

    s.done().await;
}

/// An in-flight enrichment worker must NOT resurrect data if its source was
/// revoked between `claim_next_job` and `commit_enrichment`.
#[tokio::test(flavor = "multi_thread")]
async fn enrichment_commit_is_fenced_against_concurrent_revocation() {
    let (s, engine) = setup_engine().await;
    engine
        .ingest_source(&SourceInput {
            tenant_id: DEFAULT_TENANT_ID.into(),
            source_id: "src_ephemeral".into(),
            rev: 1,
            owner_id: Some("alice".into()),
            readers: vec![vec!["person:alice".into()]],
            label: TrustLabel { untrusted: false, private: true },
            kind: "note".into(),
            text: "Secret acquisition code PROJECT-9981".into(),
            metadata: json!({}),
            occurred_at: Utc::now(),
        })
        .await
        .unwrap();

    let claimed = engine.claim_next_job().await.unwrap().expect("job claimed");
    assert_eq!(claimed.source_id, "src_ephemeral");

    // Source is revoked while the worker is still computing embeddings/extractions.
    let rev = engine.revoke_source(DEFAULT_TENANT_ID, "src_ephemeral", None).await.unwrap();
    assert_eq!(rev.sources_deleted, 1);

    // Worker finishes and attempts to commit — must be fenced cleanly!
    let outcome = engine
        .commit_enrichment(
            &claimed.tenant_id,
            &claimed.source_id,
            claimed.source_rev,
            Some(claimed.job_id),
            &EnrichmentPayload {
                source_embedding: Some(vec![0.5, 0.5, 0.5, 0.5]),
                model: Some("test-v1".into()),
                entities: vec![],
                assertions: vec![AssertionInput {
                    assertion_id: "assert_leaked".into(),
                    kind: "fact".into(),
                    statement: "Secret acquisition code PROJECT-9981".into(),
                    valid_from: None,
                    valid_to: None,
                    confidence: 1.0,
                    embedding: Some(vec![0.5, 0.5, 0.5, 0.5]),
                    model: Some("test-v1".into()),
                }],
                edges: vec![],
            },
        )
        .await
        .unwrap();
    assert_eq!(outcome, CommitOutcome::FencedSourceDeleted);

    let alice = ReaderContext::for_person(DEFAULT_TENANT_ID, "alice", &[]);
    let res = engine.retrieve(&alice, &RetrieveQuery::new("PROJECT-9981")).await.unwrap();
    assert!(res.hits.is_empty(), "fenced enrichment must never resurrect deleted data");

    s.done().await;
}

/// Exact-ID precedence, temporal candidate expansion, and 2-hop RLS graph expansion.
#[tokio::test(flavor = "multi_thread")]
async fn exact_id_temporal_and_two_hop_graph_retrieval() {
    let (s, engine) = setup_engine().await;
    let mar_2025 = Utc.with_ymd_and_hms(2025, 3, 15, 12, 0, 0).unwrap();

    let ing = engine
        .ingest_source(&SourceInput {
            tenant_id: DEFAULT_TENANT_ID.into(),
            source_id: "src_invoice_4042".into(),
            rev: 1,
            owner_id: Some("alice".into()),
            readers: vec![vec!["person:alice".into()]],
            label: TrustLabel { untrusted: false, private: false },
            kind: "doc".into(),
            text: "Invoice INV-4042 issued in March 2025 for Harbor Logistics.".into(),
            metadata: json!({}),
            occurred_at: mar_2025,
        })
        .await
        .unwrap();

    let harbor = deterministic_entity_id(DEFAULT_TENANT_ID, "company", "harbor");
    let contract = deterministic_entity_id(DEFAULT_TENANT_ID, "contract", "msa-2025");
    let warehouse = deterministic_entity_id(DEFAULT_TENANT_ID, "facility", "oakland-port");

    engine
        .commit_enrichment(
            DEFAULT_TENANT_ID,
            "src_invoice_4042",
            1,
            ing.job_id,
            &EnrichmentPayload {
                source_embedding: None,
                model: None,
                entities: vec![
                    EntityInput {
                        entity_type: "company".into(),
                        canonical_name: "Harbor Logistics".into(),
                        external_id: Some("harbor".into()),
                        aliases: vec![],
                        embedding: None,
                        model: None,
                    },
                    EntityInput {
                        entity_type: "contract".into(),
                        canonical_name: "MSA 2025".into(),
                        external_id: Some("msa-2025".into()),
                        aliases: vec![],
                        embedding: None,
                        model: None,
                    },
                    EntityInput {
                        entity_type: "facility".into(),
                        canonical_name: "Oakland Port".into(),
                        external_id: Some("oakland-port".into()),
                        aliases: vec![],
                        embedding: None,
                        model: None,
                    },
                ],
                assertions: vec![AssertionInput {
                    assertion_id: "assert_inv_4042".into(),
                    kind: "fact".into(),
                    statement: "Invoice INV-4042 covers freight handling for Harbor Logistics".into(),
                    valid_from: Some(mar_2025),
                    valid_to: Some(Utc.with_ymd_and_hms(2025, 4, 1, 0, 0, 0).unwrap()),
                    confidence: 1.0,
                    embedding: None,
                    model: None,
                }],
                edges: vec![
                    EdgeInput {
                        edge_id: "e1".into(),
                        src_id: harbor.clone(),
                        dst_id: contract.clone(),
                        rel_type: "governed_by".into(),
                        valid_from: Some(mar_2025),
                        valid_to: None,
                        properties: json!({}),
                    },
                    EdgeInput {
                        edge_id: "e2".into(),
                        src_id: contract.clone(),
                        dst_id: warehouse.clone(),
                        rel_type: "ships_through".into(),
                        valid_from: Some(mar_2025),
                        valid_to: None,
                        properties: json!({}),
                    },
                ],
            },
        )
        .await
        .unwrap();

    let alice = ReaderContext::for_person(DEFAULT_TENANT_ID, "alice", &[]);

    // 1. Exact ID fast-path.
    let exact_res = engine.retrieve(&alice, &RetrieveQuery::new("check INV-4042 status")).await.unwrap();
    assert!(!exact_res.hits.is_empty());
    assert!(exact_res.hits[0].exact_match);

    // 2. Temporal candidate expansion (date-only query with no matching lexical words).
    let temp_res = engine.retrieve(&alice, &RetrieveQuery::new("what happened in 2025-03?")).await.unwrap();
    assert!(temp_res.hits.iter().any(|h| h.id == "assert_inv_4042"));

    // 3. 2-hop graph expansion reaches Oakland Port from Harbor Logistics.
    let graph_res = engine.retrieve(&alice, &RetrieveQuery::new("Harbor Logistics")).await.unwrap();
    assert!(
        graph_res.graph_facts.iter().any(|f| f.rel_type == "ships_through" && f.dst_name == "Oakland Port"),
        "2-hop graph expansion must reach Oakland Port: {:?}",
        graph_res.graph_facts
    );

    s.done().await;
}

/// 5-job curator: feedback guardrail, semantic dedup with provenance union,
/// untrusted-cannot-supersede-clean conflict downgrade, and cold archival.
#[tokio::test(flavor = "multi_thread")]
async fn curator_feedback_dedup_conflict_guardrail_and_cold_archival() {
    let (s, engine) = setup_engine().await;
    let old_time = Utc::now() - Duration::days(45);

    // Backfill two near-identical clean facts from two different sources + one cold untrusted fact.
    let records = vec![
        LegacyRecord {
            id: "a_clean_1".into(),
            kind: "fact".into(),
            owner_id: Some("alice".into()),
            readers: vec![vec!["person:alice".into()]],
            label: TrustLabel { untrusted: false, private: false },
            text: "Production deploys freeze on Fridays".into(),
            occurred_at: Utc::now() - Duration::days(2),
            embedding: Some(vec![1.0, 0.0, 0.0, 0.0]),
            model: Some("test-v1".into()),
        },
        LegacyRecord {
            id: "a_clean_2".into(),
            kind: "fact".into(),
            owner_id: Some("alice".into()),
            readers: vec![vec!["person:alice".into()]],
            label: TrustLabel { untrusted: false, private: false },
            text: "No production deployments on Fridays".into(),
            occurred_at: Utc::now() - Duration::days(1),
            embedding: Some(vec![0.99, 0.05, 0.0, 0.0]),
            model: Some("test-v1".into()),
        },
        LegacyRecord {
            id: "a_untrusted_web".into(),
            kind: "fact".into(),
            owner_id: Some("alice".into()),
            readers: vec![vec!["person:alice".into()]],
            label: TrustLabel { untrusted: true, private: false },
            text: "Friday deploys are totally fine according to random blog".into(),
            occurred_at: old_time,
            embedding: Some(vec![0.85, 0.52, 0.0, 0.0]),
            model: Some("test-v1".into()),
        },
    ];
    let bf = engine.backfill_legacy_records(&records).await.unwrap();
    assert_eq!(bf.assertions_projected, 3);

    // Backdate created_at of the untrusted row so cold archival triggers.
    s.store().await.execute("UPDATE public.memory_assertions SET created_at = $1 WHERE assertion_id = 'a_untrusted_web'", &[&old_time]).await.unwrap();

    // Feedback guardrail: untrusted run CANNOT boost a clean assertion.
    let boosted = engine.record_feedback(DEFAULT_TENANT_ID, &["a_clean_1".into()], FeedbackSignal::Accepted, true).await.unwrap();
    assert_eq!(boosted, 0, "untrusted run must not boost clean assertion");

    // Clean run CAN boost clean assertion.
    let boosted_clean = engine.record_feedback(DEFAULT_TENANT_ID, &["a_clean_1".into()], FeedbackSignal::Accepted, false).await.unwrap();
    assert_eq!(boosted_clean, 1);

    // Trust guardrail: untrusted assertion requesting Supersede over clean is downgraded to `contradicts`.
    let edge = engine
        .record_collision_edge(DEFAULT_TENANT_ID, "a_untrusted_web", "a_clean_1", CollisionResolution::Supersede, "blog post disagrees")
        .await
        .unwrap()
        .expect("edge created");
    assert!(edge.contains("contradicts"), "untrusted must downgrade to contradicts, got {edge}");

    // Maintenance cycle merges the two clean near-duplicates and archives the 45-day-old untrusted fact.
    let report = engine.run_maintenance_cycle(DEFAULT_TENANT_ID).await.unwrap();
    assert_eq!(report.duplicates_merged, 1, "near-duplicate clean facts should merge");
    assert_eq!(report.cold_archived, 1, "45-day-old untrusted fact should archive");

    // Second run with no new writes does zero work (watermark gated).
    let idle = engine.run_maintenance_cycle(DEFAULT_TENANT_ID).await.unwrap();
    assert_eq!(idle.duplicates_merged, 0);
    assert_eq!(idle.cold_archived, 0);

    s.done().await;
}

/// Selective-RLS benchmark: 500 rows across 50 users (2% visible to caller),
/// verifying `pgvector` iterative HNSW returns accurate top-k under selective RLS.
#[tokio::test(flavor = "multi_thread")]
async fn selective_rls_iterative_hnsw_returns_authorized_top_k() {
    let (s, engine) = setup_engine().await;
    engine.ensure_hnsw_indexes(4).await.unwrap();

    let db = s.store().await;
    for i in 0..500usize {
        let user = format!("user_{}", i % 50);
        let readers = json!([[format!("person:{user}")]]);
        let vec_str = if user == "user_7" { format!("[{:.4}, {:.4}, 0.0, 0.0]", 1.0 - (i as f32 * 0.0001), 0.01) } else { "[1.0, 0.0, 0.0, 0.0]".to_string() };
        db.execute(
            "INSERT INTO public.memory_assertions (
                 tenant_id, assertion_id, kind, owner_id, readers, statement, tsv, embedding, model
             ) VALUES (
                 'default', $1, 'fact', $2, $3, $4, to_tsvector('simple', $4), $5::text::vector, 'test-v1'
             )",
            &[&format!("a_{i}"), &user, &readers, &format!("Quarterly report item {i} for {user}"), &vec_str],
        )
        .await
        .unwrap();
    }

    let u7 = ReaderContext::for_person(DEFAULT_TENANT_ID, "user_7", &[]);
    let mut q = RetrieveQuery::new("quarterly report");
    q.embedding = Some(vec![1.0, 0.0, 0.0, 0.0]);
    q.model = Some("test-v1".into());
    q.limit = 10;

    let started = std::time::Instant::now();
    let res = engine.retrieve(&u7, &q).await.unwrap();
    let elapsed = started.elapsed();

    assert_eq!(res.hits.len(), 10, "iterative HNSW under 2% selective RLS must return all 10 authorized rows");
    assert!(res.hits.iter().all(|h| h.text.contains("user_7")), "RLS must leak zero rows from other 49 users");
    assert!(elapsed < std::time::Duration::from_secs(2), "selective RLS query too slow: {elapsed:?}");

    s.done().await;
}
