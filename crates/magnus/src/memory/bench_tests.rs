//! Phase 1 unit tests and live Postgres RLS, erasure, taint-merge, dedup,
//! lease-recovery, and fencing integration tests.

use super::*;
use crate::memory::enrich::{claim_next_job, commit_enrichment, EnrichCommit, EnrichmentOutput};
use crate::memory::ingest::{ingest_source, IngestOutcome};
use crate::memory::schema::{check_pgvector_version, migrate_if_enabled, migrate_sql};
use crate::memory::store::MemoryStore;
use crate::memory_index::{HnswIterativeScan, MemoryConfig, MemoryMode, PROFILE_MAX_BYTES};
use chrono::Utc;
use serde_json::json;

const TEST_EMBEDDING_DIM: usize = 4;

fn test_config() -> MemoryConfig {
    MemoryConfig { mode: MemoryMode::V2, embedding_dim: TEST_EMBEDDING_DIM, hnsw_iterative_scan: HnswIterativeScan::RelaxedOrder, ..Default::default() }
}

#[test]
fn config_validation_rejects_invalid_dim_and_unknown_scan_mode() {
    let mut cfg = MemoryConfig::default();
    assert!(cfg.check().is_ok());
    cfg.embedding_dim = 0;
    assert!(cfg.check().is_err());
    cfg.embedding_dim = 4001;
    assert!(cfg.check().is_err());

    let bad_toml = r#"hnsw_iterative_scan = "invalid_mode""#;
    assert!(toml::from_str::<MemoryConfig>(bad_toml).is_err());

    let ok_toml = r#"hnsw_iterative_scan = "strict_order""#;
    let parsed: MemoryConfig = toml::from_str(ok_toml).unwrap();
    assert_eq!(parsed.hnsw_iterative_scan, HnswIterativeScan::StrictOrder);
}

#[test]
fn pgvector_version_check_enforces_minimum_0_8_0() {
    assert!(check_pgvector_version("0.8.0").is_ok());
    assert!(check_pgvector_version("0.8.7").is_ok());
    assert!(check_pgvector_version("0.7.4").is_err());
    assert!(check_pgvector_version("0.5.1").is_err());
}

#[test]
fn canonical_entity_id_prefers_external_id_and_normalizes_name() {
    let by_ext_a = EntityInput { entity_type: "Supplier".into(), display_name: "Secret Alias".into(), external_id: Some("SUP-42".into()) };
    let by_ext_b = EntityInput { entity_type: "supplier".into(), display_name: "Acme Corp".into(), external_id: Some("sup-42".into()) };
    assert_eq!(canonical_entity_id("t1", &by_ext_a), canonical_entity_id("t1", &by_ext_b));

    let by_name_a = EntityInput { entity_type: "Person".into(), display_name: "  Alice   Smith ".into(), external_id: None };
    let by_name_b = EntityInput { entity_type: "person".into(), display_name: "alice smith".into(), external_id: None };
    assert_eq!(canonical_entity_id("t1", &by_name_a), canonical_entity_id("t1", &by_name_b));
    assert_ne!(canonical_entity_id("t1", &by_name_a), canonical_entity_id("t2", &by_name_a));
}

#[test]
fn readers_canonicalization_and_intersection_are_order_independent() {
    let p1 = vec![vec!["user:bob".into(), "user:alice".into()], vec!["org:acme".into()]];
    let p2 = vec![vec!["org:acme".into()], vec!["user:alice".into(), "user:bob".into()]];
    assert_eq!(canonicalize_readers(&p1), canonicalize_readers(&p2));

    let merged = intersect_readers(&[vec![vec!["org:acme".into()]], vec![vec!["user:alice".into()]]]);
    assert_eq!(merged, vec![vec!["org:acme".to_string()], vec!["user:alice".to_string()]]);
}

#[test]
fn migrate_sql_enforces_forced_rls_on_all_tables_and_profile_size_cap() {
    let sql = migrate_sql("magnus_test", 1536);
    assert!(sql.contains("halfvec(1536)"));
    assert!(sql.contains("halfvec_cosine_ops"));
    assert!(sql.contains(&format!("octet_length(statement) <= {PROFILE_MAX_BYTES}")));
    assert!(sql.contains("ALTER TABLE public.memory_enrichment_jobs FORCE ROW LEVEL SECURITY"));
    assert!(!sql.contains("deleted_at"));
    assert!(!sql.contains("canonical_name"));
}

/// Connect to live Postgres when `MAGNUS_TEST_ADMIN_URL` and `MAGNUS_TEST_KERNEL_URL`
/// are set. Panics loudly on connection/migration failure if env vars are present.
async fn setup_test_store() -> Option<(String, String, MemoryStore)> {
    let admin_url = std::env::var("MAGNUS_TEST_ADMIN_URL").ok()?;
    let kernel_url = std::env::var("MAGNUS_TEST_KERNEL_URL").expect("MAGNUS_TEST_KERNEL_URL required with MAGNUS_TEST_ADMIN_URL");
    let parsed = url::Url::parse(&kernel_url).expect("valid MAGNUS_TEST_KERNEL_URL");
    let role = parsed.username().to_string();
    let admin = crate::log::connect(&admin_url).await.expect("connect MAGNUS_TEST_ADMIN_URL");
    let _ = admin
        .batch_execute(&format!(
            "DO $$ BEGIN CREATE ROLE {role} LOGIN PASSWORD 'testpw' NOSUPERUSER NOBYPASSRLS; \
             EXCEPTION WHEN duplicate_object THEN NULL; END $$;"
        ))
        .await;
    let cfg = test_config();
    migrate_if_enabled(&admin_url, &kernel_url, &cfg).await.expect("migrate_if_enabled");
    let store = MemoryStore::connect(&kernel_url, &cfg).await.expect("connect MemoryStore");
    Some((admin_url, kernel_url, store))
}

#[tokio::test]
async fn phase1_live_rls_erasure_taint_merge_dedup_and_fencing() {
    let Some((admin_url, kernel_url, mut store)) = setup_test_store().await else {
        return;
    };
    let tenant = format!("t_{}", Uuid::new_v4());
    let now = Utc::now();

    // 0. Legacy mode migration is a no-op even with arbitrary dim.
    let legacy_cfg = MemoryConfig { mode: MemoryMode::Legacy, embedding_dim: 128, ..Default::default() };
    migrate_if_enabled(&admin_url, &kernel_url, &legacy_cfg).await.unwrap();

    // 1. Order-independent readers dedup on same source_id, while different source_id stays independent.
    let src_a = SourceInput {
        tenant_id: tenant.clone(),
        source_id: "email_a".into(),
        rev: 1,
        owner_id: "alice".into(),
        readers: vec![vec!["user:alice".into()], vec!["org:acme".into()]],
        label: TaintLabel { untrusted: false, private: true },
        text: "Supplier Acme lead time is 6 weeks.".into(),
        occurred_at: now,
    };
    assert!(matches!(ingest_source(&mut store, &src_a).await.unwrap(), IngestOutcome::Inserted { .. }));

    let mut src_a_reordered = src_a.clone();
    src_a_reordered.rev = 2;
    src_a_reordered.readers = vec![vec!["org:acme".into()], vec!["user:alice".into()]];
    assert_eq!(ingest_source(&mut store, &src_a_reordered).await.unwrap(), IngestOutcome::Deduplicated { existing_source_id: "email_a".into(), existing_rev: 1 });

    let mut src_b = src_a.clone();
    src_b.source_id = "email_b".into();
    assert!(matches!(ingest_source(&mut store, &src_b).await.unwrap(), IngestOutcome::Inserted { .. }));

    // 2. Enrich email_a with a shared external_id entity using Alice's private display_name,
    //    plus a multi-source distilled lesson and profile entry.
    let job_a = claim_next_job(&mut store).await.unwrap().expect("job_a");
    let job_b = claim_next_job(&mut store).await.unwrap().expect("job_b");
    let out_a = EnrichmentOutput {
        assertions: vec![
            AssertionInput {
                assertion_id: "a_lesson".into(),
                kind: AssertionKind::Lesson,
                owner_id: "alice".into(),
                statement: "Always verify Acme lead time".into(),
                valid_from: now,
                valid_to: None,
                confidence: 0.9,
                embedding: Some(vec![0.1, 0.2, 0.3, 0.4]),
            },
            AssertionInput {
                assertion_id: "a_profile".into(),
                kind: AssertionKind::Profile,
                owner_id: "alice".into(),
                statement: "Prefer concise status updates".into(),
                valid_from: now,
                valid_to: None,
                confidence: 1.0,
                embedding: None,
            },
        ],
        edges: vec![EdgeInput {
            edge_id: "e_alice".into(),
            src: EntityInput { entity_type: "supplier".into(), display_name: "Secret Project X".into(), external_id: Some("SUP-1".into()) },
            dst: EntityInput { entity_type: "metric".into(), display_name: "6 weeks".into(), external_id: None },
            rel_type: "lead_time".into(),
            valid_from: now,
            valid_to: None,
            properties: json!({}),
        }],
    };
    assert_eq!(commit_enrichment(&mut store, &job_a, &out_a).await.unwrap(), EnrichCommit::Committed);

    // Attach email_b as a second evidence source to a_lesson & a_profile.
    let out_b = EnrichmentOutput {
        assertions: vec![
            AssertionInput {
                assertion_id: "a_lesson".into(),
                kind: AssertionKind::Lesson,
                owner_id: "alice".into(),
                statement: "Always verify Acme lead time".into(),
                valid_from: now,
                valid_to: None,
                confidence: 0.9,
                embedding: None,
            },
            AssertionInput {
                assertion_id: "a_profile".into(),
                kind: AssertionKind::Profile,
                owner_id: "alice".into(),
                statement: "Prefer concise status updates".into(),
                valid_from: now,
                valid_to: None,
                confidence: 1.0,
                embedding: None,
            },
        ],
        edges: vec![],
    };
    assert_eq!(commit_enrichment(&mut store, &job_b, &out_b).await.unwrap(), EnrichCommit::Committed);

    // 3. Cross-ACL entity name isolation: Bob writes a shared edge referencing SUP-1 as "Acme Corp";
    //    Bob must NEVER see Alice's "Secret Project X" name or edge.
    let src_bob = SourceInput {
        tenant_id: tenant.clone(),
        source_id: "shared_doc".into(),
        rev: 1,
        owner_id: "bob".into(),
        readers: vec![vec!["org:acme".into()]],
        label: TaintLabel { untrusted: false, private: false },
        text: "Acme Corp standard lead time is 4 weeks.".into(),
        occurred_at: now,
    };
    assert!(matches!(ingest_source(&mut store, &src_bob).await.unwrap(), IngestOutcome::Inserted { .. }));
    let job_bob = claim_next_job(&mut store).await.unwrap().expect("job_bob");
    let out_bob = EnrichmentOutput {
        assertions: vec![AssertionInput {
            assertion_id: "a_shared".into(),
            kind: AssertionKind::Fact,
            owner_id: "bob".into(),
            statement: "Acme lead time is 4 weeks".into(),
            valid_from: now,
            valid_to: None,
            confidence: 0.9,
            embedding: Some(vec![0.4, 0.3, 0.2, 0.1]),
        }],
        edges: vec![EdgeInput {
            edge_id: "e_bob".into(),
            src: EntityInput { entity_type: "supplier".into(), display_name: "Acme Corp".into(), external_id: Some("SUP-1".into()) },
            dst: EntityInput { entity_type: "metric".into(), display_name: "4 weeks".into(), external_id: None },
            rel_type: "lead_time".into(),
            valid_from: now,
            valid_to: None,
            properties: json!({}),
        }],
    };
    assert_eq!(commit_enrichment(&mut store, &job_bob, &out_bob).await.unwrap(), EnrichCommit::Committed);
    {
        let tx = store.begin_reader_tx(&["org:acme".into(), "user:bob".into()]).await.unwrap();
        let names: Vec<String> = tx.query("SELECT src_name FROM public.memory_edges WHERE tenant_id = $1", &[&tenant]).await.unwrap().into_iter().map(|r| r.get(0)).collect();
        assert_eq!(names, vec!["Acme Corp".to_string()], "bob must not see alice's private entity name");
        tx.commit().await.unwrap();
    }

    // 4. Re-using existing assertion with untrusted + narrower readers joins taint & narrows readers.
    let src_untrusted = SourceInput {
        tenant_id: tenant.clone(),
        source_id: "web_1".into(),
        rev: 1,
        owner_id: "bob".into(),
        readers: vec![vec!["user:bob".into()]],
        label: TaintLabel { untrusted: true, private: false },
        text: "External page confirming 4 weeks.".into(),
        occurred_at: now,
    };
    assert!(matches!(ingest_source(&mut store, &src_untrusted).await.unwrap(), IngestOutcome::Inserted { .. }));
    let job_u = claim_next_job(&mut store).await.unwrap().expect("job_u");
    let out_u = EnrichmentOutput {
        assertions: vec![AssertionInput {
            assertion_id: "a_shared".into(),
            kind: AssertionKind::Fact,
            owner_id: "bob".into(),
            statement: "Acme lead time is 4 weeks".into(),
            valid_from: now,
            valid_to: None,
            confidence: 0.5,
            embedding: None,
        }],
        edges: vec![],
    };
    assert_eq!(commit_enrichment(&mut store, &job_u, &out_u).await.unwrap(), EnrichCommit::Committed);
    let row = store.client().query_one("SELECT untrusted, readers FROM public.memory_assertions WHERE tenant_id = $1 AND assertion_id = 'a_shared'", &[&tenant]).await.unwrap();
    assert!(row.get::<_, bool>(0), "re-using assertion with untrusted source must taint it");
    let merged_readers: Vec<Vec<String>> = serde_json::from_value(row.get::<_, serde_json::Value>(1)).unwrap();
    assert_eq!(merged_readers, vec![vec!["org:acme".to_string()], vec!["user:bob".to_string()]]);

    // 5. Profile > 8 KB rejected.
    let src_big = SourceInput {
        tenant_id: tenant.clone(),
        source_id: "big_profile".into(),
        rev: 1,
        owner_id: "alice".into(),
        readers: vec![vec!["user:alice".into()]],
        label: TaintLabel { untrusted: false, private: true },
        text: "Big profile source".into(),
        occurred_at: now,
    };
    assert!(matches!(ingest_source(&mut store, &src_big).await.unwrap(), IngestOutcome::Inserted { .. }));
    let job_big = claim_next_job(&mut store).await.unwrap().expect("job_big");
    let out_big = EnrichmentOutput {
        assertions: vec![AssertionInput {
            assertion_id: "a_big_profile".into(),
            kind: AssertionKind::Profile,
            owner_id: "alice".into(),
            statement: "x".repeat(PROFILE_MAX_BYTES + 1),
            valid_from: now,
            valid_to: None,
            confidence: 1.0,
            embedding: None,
        }],
        edges: vec![],
    };
    assert!(commit_enrichment(&mut store, &job_big, &out_big).await.is_err());

    // 6. Erasure of email_a hard-deletes multi-source a_lesson & a_profile, purges
    //    their remaining email_b evidence, and sweeps Alice's orphaned "6 weeks" entity.
    assert_eq!(store.hard_delete_source_rev(&tenant, "email_a", 1).await.unwrap(), 1);
    let remaining_derived: i64 = store
        .client()
        .query_one("SELECT count(*) FROM public.memory_assertions WHERE tenant_id = $1 AND assertion_id IN ('a_lesson', 'a_profile')", &[&tenant])
        .await
        .unwrap()
        .get(0);
    assert_eq!(remaining_derived, 0, "multi-source lesson/profile must be hard-deleted on single source erasure");

    let orphan_ev: i64 =
        store.client().query_one("SELECT count(*) FROM public.memory_evidence WHERE tenant_id = $1 AND target_id IN ('a_lesson', 'a_profile')", &[&tenant]).await.unwrap().get(0);
    assert_eq!(orphan_ev, 0, "no orphan evidence rows may remain for deleted lessons/profiles");

    let orphan_metric_id = canonical_entity_id(&tenant, &EntityInput { entity_type: "metric".into(), display_name: "6 weeks".into(), external_id: None });
    let orphan_ent: i64 =
        store.client().query_one("SELECT count(*) FROM public.memory_entities WHERE tenant_id = $1 AND entity_id = $2", &[&tenant, &orphan_metric_id]).await.unwrap().get(0);
    assert_eq!(orphan_ent, 0, "orphaned memory_entities must be deleted when their last edge is erased");

    // 7. Expired `running` job lease is reclaimed by claim_next_job.
    store
        .client()
        .execute("UPDATE public.memory_enrichment_jobs SET locked_until = now() - interval '10 seconds' WHERE tenant_id = $1 AND job_id = $2", &[&tenant, &job_big.job_id])
        .await
        .unwrap();
    let reclaimed = claim_next_job(&mut store).await.unwrap().expect("expired lease must be reclaimed");
    assert_eq!(reclaimed.job_id, job_big.job_id);
    assert_eq!(reclaimed.attempts, 2);

    // 8. Concurrent revocation fencing: hard-delete mid-flight causes commit_enrichment to roll back.
    assert_eq!(store.hard_delete_source_rev(&tenant, "big_profile", 1).await.unwrap(), 1);
    let out_small = EnrichmentOutput {
        assertions: vec![AssertionInput {
            assertion_id: "a_fenced".into(),
            kind: AssertionKind::Fact,
            owner_id: "alice".into(),
            statement: "Should never commit".into(),
            valid_from: now,
            valid_to: None,
            confidence: 1.0,
            embedding: None,
        }],
        edges: vec![],
    };
    assert_eq!(commit_enrichment(&mut store, &reclaimed, &out_small).await.unwrap(), EnrichCommit::FencedDeleted);
}
