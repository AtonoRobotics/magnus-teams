//! Phase 1 unit tests and live Postgres RLS, erasure, taint-merge, dedup,
//! lease-recovery, and fencing integration tests.

use super::*;
use crate::memory::enrich::{claim_next_job, commit_enrichment, EnrichCommit, EnrichmentOutput};
use crate::memory::ingest::{ingest_source, IngestOutcome};
use crate::memory::live_pg_test::{test_config, LiveDb};
use crate::memory::schema::{check_pgvector_version, migrate_if_enabled, migrate_sql};
use crate::memory::store::{SupersessionOutcome, SupersessionRefusal};
use crate::memory_index::{HnswIterativeScan, MemoryConfig, MemoryMode, PROFILE_MAX_BYTES};
use chrono::Utc;
use serde_json::json;

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

#[tokio::test(flavor = "multi_thread")]
async fn phase1_live_rls_erasure_taint_merge_dedup_and_fencing() {
    let db = LiveDb::create().await;
    let (admin_url, kernel_url) = (db.admin_url.clone(), db.kernel_url.clone());
    let mut store = db.store().await;
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
        let tx = store.begin_reader_tx(&tenant, &["org:acme".into(), "user:bob".into()]).await.unwrap();
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

#[tokio::test]
async fn phase2_mixed_visibility_override_hop_taint_hybrid_and_selective_rls() {
    use crate::memory::retrieve::{retrieve, HitOrigin, RetrieveRequest};
    use chrono::Duration;
    use std::time::Instant;

    let db = LiveDb::create().await;
    let mut store = db.store().await;
    let tenant = format!("t_{}", Uuid::new_v4());
    let now = Utc::now();

    // ------------------------------------------------------------------
    // 1. Mixed-visibility override & revocation (Phase 0 acceptance test)
    // ------------------------------------------------------------------
    let shared_src = SourceInput {
        tenant_id: tenant.clone(),
        source_id: "kb_supplier".into(),
        rev: 1,
        owner_id: "ops".into(),
        readers: vec![vec!["org:acme".into()]],
        label: TaintLabel { untrusted: false, private: false },
        text: "Supplier Acme standard lead time is 4 weeks.".into(),
        occurred_at: now - Duration::days(10),
    };
    assert!(matches!(ingest_source(&mut store, &shared_src).await.unwrap(), IngestOutcome::Inserted { .. }));
    let j1 = claim_next_job(&mut store).await.unwrap().expect("shared job");
    assert_eq!(
        commit_enrichment(
            &mut store,
            &j1,
            &EnrichmentOutput {
                assertions: vec![AssertionInput {
                    assertion_id: "a_lead_shared".into(),
                    kind: AssertionKind::Fact,
                    owner_id: "ops".into(),
                    statement: "Supplier Acme lead time is 4 weeks".into(),
                    valid_from: now - Duration::days(10),
                    valid_to: None,
                    confidence: 0.95,
                    embedding: Some(vec![0.9, 0.1, 0.0, 0.0]),
                }],
                edges: vec![EdgeInput {
                    edge_id: "e_acme_hq".into(),
                    src: EntityInput { entity_type: "supplier".into(), display_name: "Acme".into(), external_id: Some("SUP-ACME".into()) },
                    dst: EntityInput { entity_type: "city".into(), display_name: "Austin".into(), external_id: None },
                    rel_type: "headquartered_in".into(),
                    valid_from: now - Duration::days(10),
                    valid_to: None,
                    properties: json!({}),
                }],
            },
        )
        .await
        .unwrap(),
        EnrichCommit::Committed
    );

    // Buyer's private email overrides lead time to 6 weeks.
    let buyer_src = SourceInput {
        tenant_id: tenant.clone(),
        source_id: "buyer_email".into(),
        rev: 1,
        owner_id: "buyer".into(),
        readers: vec![vec!["user:buyer".into()]],
        label: TaintLabel { untrusted: false, private: true },
        text: "Private email: Supplier Acme lead time increased to 6 weeks.".into(),
        occurred_at: now,
    };
    assert!(matches!(ingest_source(&mut store, &buyer_src).await.unwrap(), IngestOutcome::Inserted { .. }));
    let j2 = claim_next_job(&mut store).await.unwrap().expect("buyer job");
    assert_eq!(
        commit_enrichment(
            &mut store,
            &j2,
            &EnrichmentOutput {
                assertions: vec![AssertionInput {
                    assertion_id: "a_lead_private".into(),
                    kind: AssertionKind::Fact,
                    owner_id: "buyer".into(),
                    statement: "Supplier Acme lead time is 6 weeks".into(),
                    valid_from: now,
                    valid_to: None,
                    confidence: 1.0,
                    embedding: Some(vec![0.92, 0.08, 0.0, 0.0]),
                }],
                edges: vec![],
            },
        )
        .await
        .unwrap(),
        EnrichCommit::Committed
    );
    store.record_supersession(&tenant, "a_lead_private", "a_lead_shared").await.unwrap();

    // Buyer sees 6 weeks (and NOT 4 weeks); private taint is propagated.
    let mut buyer_req = RetrieveRequest::new(&tenant, vec!["org:acme".into(), "user:buyer".into()], "Acme lead time");
    buyer_req.query_embedding = Some(vec![0.91, 0.09, 0.0, 0.0]);
    let buyer_res = retrieve(&mut store, &buyer_req).await.unwrap();
    let buyer_ids: Vec<&str> = buyer_res.hits.iter().map(|h| h.id.as_str()).collect();
    assert!(buyer_ids.contains(&"a_lead_private"), "buyer must see private override: {buyer_ids:?}");
    assert!(!buyer_ids.contains(&"a_lead_shared"), "buyer must not see superseded shared assertion: {buyer_ids:?}");
    assert!(buyer_res.label.private && !buyer_res.label.untrusted);

    // Teammate sees 4 weeks with zero leakage of buyer's private override.
    let mut mate_req = RetrieveRequest::new(&tenant, vec!["org:acme".into(), "user:teammate".into()], "Acme lead time");
    mate_req.query_embedding = Some(vec![0.91, 0.09, 0.0, 0.0]);
    let mate_res = retrieve(&mut store, &mate_req).await.unwrap();
    let mate_ids: Vec<&str> = mate_res.hits.iter().map(|h| h.id.as_str()).collect();
    assert!(mate_ids.contains(&"a_lead_shared"), "teammate must still see shared assertion: {mate_ids:?}");
    assert!(!mate_ids.contains(&"a_lead_private"), "teammate must never see buyer override");
    assert!(!mate_res.label.private && !mate_res.label.untrusted);

    // Revoking buyer's email cascades memory_supersessions and restores 4 weeks for buyer.
    assert_eq!(store.hard_delete_source_rev(&tenant, "buyer_email", 1).await.unwrap(), 1);
    let buyer_after = retrieve(&mut store, &buyer_req).await.unwrap();
    let buyer_after_ids: Vec<&str> = buyer_after.hits.iter().map(|h| h.id.as_str()).collect();
    assert!(buyer_after_ids.contains(&"a_lead_shared"), "revoking private override must restore shared view");
    assert!(!buyer_after_ids.contains(&"a_lead_private"));

    // ------------------------------------------------------------------
    // 2. Hop-level taint isolation + 2-hop graph expansion + sync fallback
    // ------------------------------------------------------------------
    let untrusted_src = SourceInput {
        tenant_id: tenant.clone(),
        source_id: "web_rumor".into(),
        rev: 1,
        owner_id: "ops".into(),
        readers: vec![vec!["org:acme".into()]],
        label: TaintLabel { untrusted: true, private: false },
        text: "External blog rumor: Austin port is congested.".into(),
        occurred_at: now,
    };
    assert!(matches!(ingest_source(&mut store, &untrusted_src).await.unwrap(), IngestOutcome::Inserted { .. }));
    let j3 = claim_next_job(&mut store).await.unwrap().expect("untrusted job");
    assert_eq!(
        commit_enrichment(
            &mut store,
            &j3,
            &EnrichmentOutput {
                assertions: vec![],
                edges: vec![EdgeInput {
                    edge_id: "e_austin_port".into(),
                    src: EntityInput { entity_type: "city".into(), display_name: "Austin".into(), external_id: None },
                    dst: EntityInput { entity_type: "event".into(), display_name: "Port Congestion".into(), external_id: None },
                    rel_type: "affected_by".into(),
                    valid_from: now,
                    valid_to: None,
                    properties: json!({}),
                }],
            },
        )
        .await
        .unwrap(),
        EnrichCommit::Committed
    );

    // Clean-only read (allow_untrusted = false): 1-hop reaches Austin, must NOT traverse untrusted hop-2 edge.
    let clean_req = RetrieveRequest::new(&tenant, vec!["org:acme".into()], "SUP-ACME");
    let clean_res = retrieve(&mut store, &clean_req).await.unwrap();
    assert!(clean_res.edges.iter().any(|e| e.edge_id == "e_acme_hq"));
    assert!(!clean_res.edges.iter().any(|e| e.edge_id == "e_austin_port"), "clean read must prune untrusted 2-hop edge");
    assert!(!clean_res.label.untrusted);

    // Normal read (allow_untrusted = true): 2-hop traverses untrusted edge and flips taint bit.
    let mut normal_req = RetrieveRequest::new(&tenant, vec!["org:acme".into()], "SUP-ACME");
    normal_req.allow_untrusted = true;
    let normal_res = retrieve(&mut store, &normal_req).await.unwrap();
    assert!(normal_res.edges.iter().any(|e| e.edge_id == "e_austin_port" && e.hop == 2));
    assert!(normal_res.label.untrusted, "traversing untrusted edge must flip label.untrusted");

    // Unenriched freshly-written source is immediately recallable via sync FTS fallback.
    let fresh_src = SourceInput {
        tenant_id: tenant.clone(),
        source_id: "fresh_note".into(),
        rev: 1,
        owner_id: "ops".into(),
        readers: vec![vec!["org:acme".into()]],
        label: TaintLabel { untrusted: false, private: false },
        text: "Ticket INV-99421 approved for Q4 shipment.".into(),
        occurred_at: now,
    };
    assert!(matches!(ingest_source(&mut store, &fresh_src).await.unwrap(), IngestOutcome::Inserted { .. }));
    let fresh_req = RetrieveRequest::new(&tenant, vec!["org:acme".into()], "INV-99421");
    let fresh_res = retrieve(&mut store, &fresh_req).await.unwrap();
    assert!(fresh_res.hits.iter().any(|h| h.origin == HitOrigin::UnenrichedSource && h.text.contains("INV-99421")));

    // ------------------------------------------------------------------
    // 3. Selective-RLS HNSW iterative scan (1,000 rows, 1% visible)
    // ------------------------------------------------------------------
    let sel_tenant = format!("t_sel_{}", Uuid::new_v4());
    let tx = store.client_mut().transaction().await.unwrap();
    for i in 0..1000_i32 {
        let visible = i < 10;
        let readers = if visible { json!([["user:needle"]]) } else { json!([["user:other"]]) };
        let emb = if visible { "[0.99,0.01,0.0,0.0]" } else { "[1.0,0.0,0.0,0.0]" };
        let aid = format!("sel_{i}");
        let stmt = format!("Selective row {i} inventory status");
        tx.execute(
            "INSERT INTO public.memory_assertions (
                 tenant_id, assertion_id, kind, owner_id, readers, untrusted, private,
                 statement, valid_from, confidence, tsv, embedding
             ) VALUES (
                 $1, $2, 'fact', 'sys', $3, false, false,
                 $4, now(), 1.0, to_tsvector('simple', $4), $5::text::halfvec
             )",
            &[&sel_tenant, &aid, &readers, &stmt, &emb],
        )
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();

    let mut sel_req = RetrieveRequest::new(&sel_tenant, vec!["user:needle".into()], "nonmatching_lexical_term");
    sel_req.query_embedding = Some(vec![0.99, 0.01, 0.0, 0.0]);
    sel_req.limit = 10;
    let t0 = Instant::now();
    let sel_res = retrieve(&mut store, &sel_req).await.unwrap();
    let elapsed_ms = t0.elapsed().as_millis();
    assert_eq!(sel_res.hits.len(), 10, "iterative HNSW must recover all 10 visible rows under 1% RLS selectivity (got {}, took {elapsed_ms}ms)", sel_res.hits.len());
}

#[tokio::test]
async fn phase2_review_fixes_tenant_rls_supersession_guards_advisory_lock_and_multiword_graph() {
    use crate::memory::retrieve::{retrieve, RetrieveRequest};
    use std::time::Instant;

    const SELECTIVE_RLS_MAX_LATENCY_MS: u128 = 500;

    let db = LiveDb::create().await;
    let (admin_url, kernel_url) = (db.admin_url.clone(), db.kernel_url.clone());
    let mut store = db.store().await;

    // 1. Concurrent migrate_if_enabled must succeed without pg_type catalog race.
    let cfg = test_config();
    let mut handles = Vec::new();
    for _ in 0..4 {
        let a = admin_url.clone();
        let k = kernel_url.clone();
        let c = cfg.clone();
        handles.push(tokio::spawn(async move { migrate_if_enabled(&a, &k, &c).await }));
    }
    for h in handles {
        h.await.unwrap().expect("concurrent migrate_if_enabled must serialize via advisory lock");
    }

    // 2. DB-enforced tenant RLS: same principal `user:shared` in tenant_a and tenant_b;
    //    querying tenant_b inside a tenant_a reader tx must return 0 rows.
    let tenant_a = format!("ta_{}", Uuid::new_v4());
    let tenant_b = format!("tb_{}", Uuid::new_v4());
    let now = Utc::now();
    for (tid, sid, stmt) in [(&tenant_a, "s_a", "Tenant A secret"), (&tenant_b, "s_b", "Tenant B secret")] {
        let src = SourceInput {
            tenant_id: tid.clone(),
            source_id: sid.into(),
            rev: 1,
            owner_id: "shared".into(),
            readers: vec![vec!["user:shared".into()]],
            label: TaintLabel { untrusted: false, private: false },
            text: stmt.into(),
            occurred_at: now,
        };
        assert!(matches!(ingest_source(&mut store, &src).await.unwrap(), IngestOutcome::Inserted { .. }));
    }
    {
        let tx = store.begin_reader_tx(&tenant_a, &["user:shared".into()]).await.unwrap();
        let leaked: i64 = tx.query_one("SELECT count(*) FROM public.memory_sources WHERE tenant_id = $1", &[&tenant_b]).await.unwrap().get(0);
        assert_eq!(leaked, 0, "DB RLS must block cross-tenant reads even when principal names match");
        tx.commit().await.unwrap();
    }

    // 3. record_supersession guards: untrusted->trusted (Inv 4), cycles, non-profile->profile.
    let t = format!("tg_{}", Uuid::new_v4());
    let s_clean = SourceInput {
        tenant_id: t.clone(),
        source_id: "s_clean".into(),
        rev: 1,
        owner_id: "alice".into(),
        readers: vec![vec!["org:acme".into()]],
        label: TaintLabel { untrusted: false, private: false },
        text: "Clean fact and profile".into(),
        occurred_at: now,
    };
    let s_untrusted = SourceInput {
        tenant_id: t.clone(),
        source_id: "s_untrusted".into(),
        rev: 1,
        owner_id: "alice".into(),
        readers: vec![vec!["org:acme".into()]],
        label: TaintLabel { untrusted: true, private: false },
        text: "Untrusted claim".into(),
        occurred_at: now,
    };
    assert!(matches!(ingest_source(&mut store, &s_clean).await.unwrap(), IngestOutcome::Inserted { .. }));
    assert!(matches!(ingest_source(&mut store, &s_untrusted).await.unwrap(), IngestOutcome::Inserted { .. }));
    // Drain the queue: the tenant_a/tenant_b sources from step 2 left jobs with nothing to extract.
    while let Some(job) = claim_next_job(&mut store).await.unwrap() {
        if job.tenant_id != t {
            assert_eq!(commit_enrichment(&mut store, &job, &EnrichmentOutput::default()).await.unwrap(), EnrichCommit::Committed);
            continue;
        }
        let out = if job.source_id == "s_clean" {
            EnrichmentOutput {
                assertions: vec![
                    AssertionInput {
                        assertion_id: "a_trusted".into(),
                        kind: AssertionKind::Fact,
                        owner_id: "alice".into(),
                        statement: "Trusted policy fact".into(),
                        valid_from: now,
                        valid_to: None,
                        confidence: 1.0,
                        embedding: Some(vec![0.8, 0.6, 0.0, 0.0]),
                    },
                    AssertionInput {
                        assertion_id: "a_trusted_2".into(),
                        kind: AssertionKind::Fact,
                        owner_id: "alice".into(),
                        statement: "Trusted policy fact v2".into(),
                        valid_from: now,
                        valid_to: None,
                        confidence: 1.0,
                        embedding: None,
                    },
                    AssertionInput {
                        assertion_id: "a_prof".into(),
                        kind: AssertionKind::Profile,
                        owner_id: "alice".into(),
                        statement: "Standing profile rule".into(),
                        valid_from: now,
                        valid_to: None,
                        confidence: 1.0,
                        embedding: None,
                    },
                ],
                edges: vec![EdgeInput {
                    edge_id: "e_multi".into(),
                    src: EntityInput { entity_type: "event".into(), display_name: "Port Congestion".into(), external_id: None },
                    dst: EntityInput { entity_type: "city".into(), display_name: "Austin".into(), external_id: None },
                    rel_type: "impacts".into(),
                    valid_from: now,
                    valid_to: None,
                    properties: json!({}),
                }],
            }
        } else {
            EnrichmentOutput {
                assertions: vec![AssertionInput {
                    assertion_id: "a_untrusted".into(),
                    kind: AssertionKind::Fact,
                    owner_id: "alice".into(),
                    statement: "Untrusted override attempt".into(),
                    valid_from: now,
                    valid_to: None,
                    confidence: 0.5,
                    embedding: None,
                }],
                edges: vec![],
            }
        };
        assert_eq!(commit_enrichment(&mut store, &job, &out).await.unwrap(), EnrichCommit::Committed);
    }

    let refused = |r| SupersessionOutcome::Refused(r);
    assert_eq!(store.record_supersession(&t, "a_untrusted", "a_trusted").await.unwrap(), refused(SupersessionRefusal::UntrustedOverTrusted));
    assert_eq!(store.record_supersession(&t, "a_trusted", "a_prof").await.unwrap(), refused(SupersessionRefusal::NonProfileOverProfile));
    assert_eq!(store.record_supersession(&t, "a_trusted_2", "a_trusted").await.unwrap(), SupersessionOutcome::Recorded);
    assert_eq!(store.record_supersession(&t, "a_trusted", "a_trusted_2").await.unwrap(), refused(SupersessionRefusal::Cycle));
    assert_eq!(store.record_supersession(&t, "a_trusted", "a_trusted").await.unwrap(), refused(SupersessionRefusal::SelfLink));
    assert_eq!(store.record_supersession(&t, "a_gone", "a_trusted").await.unwrap(), refused(SupersessionRefusal::Missing));

    // 4. Multi-word entity lookup ("Port Congestion") finds the edge directly.
    let mw_req = RetrieveRequest::new(&t, vec!["org:acme".into()], "Tell me about Port Congestion");
    let mw_res = retrieve(&mut store, &mw_req).await.unwrap();
    assert!(mw_res.edges.iter().any(|e| e.edge_id == "e_multi"), "multi-word entity name must be found by n-gram seed lookup");

    // 5. Reject max_graph_hops > MAX_GRAPH_HOPS rather than silently clamping.
    let mut bad_hops = RetrieveRequest::new(&t, vec!["org:acme".into()], "Port Congestion");
    bad_hops.max_graph_hops = crate::memory::graph::MAX_GRAPH_HOPS + 1;
    assert!(retrieve(&mut store, &bad_hops).await.is_err());

    // 6. Real selective-RLS iterative HNSW test: 500 invisible rows CLOSER ([1,0,0,0])
    //    than 10 visible rows ([0.8,0.6,0,0]) with enable_seqscan = off.
    let t_sel = format!("tsel_{}", Uuid::new_v4());
    let tx = store.client_mut().transaction().await.unwrap();
    for i in 0..510_i32 {
        let visible = i < 10;
        let readers = if visible { json!([["user:needle"]]) } else { json!([["user:other"]]) };
        let emb = if visible { "[0.8,0.6,0.0,0.0]" } else { "[1.0,0.0,0.0,0.0]" };
        let aid = format!("hnsw_{i}");
        let stmt = format!("Vector row {i}");
        tx.execute(
            "INSERT INTO public.memory_assertions (
                 tenant_id, assertion_id, kind, owner_id, readers, untrusted, private,
                 statement, valid_from, confidence, tsv, embedding
             ) VALUES (
                 $1, $2, 'fact', 'sys', $3, false, false,
                 $4, now(), 1.0, to_tsvector('simple', $4), $5::text::halfvec
             )",
            &[&t_sel, &aid, &readers, &stmt, &emb],
        )
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();

    // Verify via reader tx with enable_seqscan = off and hnsw.iterative_scan = relaxed_order
    let t0 = Instant::now();
    {
        let tx = store.begin_reader_tx(&t_sel, &["user:needle".into()]).await.unwrap();
        tx.batch_execute("SET LOCAL enable_seqscan = off;").await.unwrap();
        let rows = tx
            .query(
                "SELECT assertion_id FROM public.memory_assertions
                 WHERE tenant_id = $1 AND NOT archived
                 ORDER BY embedding <=> '[1.0,0.0,0.0,0.0]'::halfvec ASC
                 LIMIT 10",
                &[&t_sel],
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 10, "iterative_scan must recover all 10 visible rows behind 500 closer invisible rows");
        tx.commit().await.unwrap();
    }
    let elapsed_ms = t0.elapsed().as_millis();
    assert!(elapsed_ms <= SELECTIVE_RLS_MAX_LATENCY_MS, "selective HNSW took {elapsed_ms}ms > {SELECTIVE_RLS_MAX_LATENCY_MS}ms");
}

#[tokio::test]
async fn phase3_review_fixes_custom_dim_record_access_and_supersession_window_cover() {
    use crate::memory::curator::archive_cold_and_sweep_orphans;
    use crate::memory::live_pg_test::LiveDb;
    use crate::memory::retrieve::{retrieve, RetrieveRequest};
    use crate::memory::store::MemoryStore;
    use crate::memory_index::{HnswIterativeScan, MemoryConfig, MemoryMode};
    use chrono::Duration;

    // 1. Non-default embedding_dim (8 != 1536) via migrate_if_enabled works end-to-end.
    let db = LiveDb::create().await;
    let custom_cfg = MemoryConfig { mode: MemoryMode::V2, embedding_dim: 8, hnsw_iterative_scan: HnswIterativeScan::RelaxedOrder, ..Default::default() };
    // Fresh separate database migrated ONLY via migrate_if_enabled with dim=8
    let server_url = std::env::var("MAGNUS_TEST_PG").unwrap_or_else(|_| "postgres://postgres:magnus_dev@127.0.0.1:54340/postgres".into());
    let server = crate::log::connect(&server_url).await.unwrap();
    let custom_name = format!("mem_dim8_{}", Uuid::new_v4().simple());
    server.batch_execute(&format!("CREATE DATABASE {custom_name}")).await.unwrap();
    server.batch_execute(&format!("CREATE ROLE {custom_name} LOGIN PASSWORD 'memtestpw' NOSUPERUSER NOBYPASSRLS")).await.unwrap();
    let mut admin8 = url::Url::parse(&server_url).unwrap();
    admin8.set_path(&format!("/{custom_name}"));
    let mut kernel8 = admin8.clone();
    kernel8.set_username(&custom_name).unwrap();
    kernel8.set_password(Some("memtestpw")).unwrap();

    migrate_if_enabled(admin8.as_str(), kernel8.as_str(), &custom_cfg).await.unwrap();
    let mut store8 = MemoryStore::connect(kernel8.as_str(), &custom_cfg).await.unwrap();
    let t8 = format!("t8_{}", Uuid::new_v4());
    let now = Utc::now();
    let s8 = SourceInput {
        tenant_id: t8.clone(),
        source_id: "s8".into(),
        rev: 1,
        owner_id: "alice".into(),
        readers: vec![vec!["user:alice".into()]],
        label: TaintLabel { untrusted: false, private: false },
        text: "Eight-dim vector fact".into(),
        occurred_at: now,
    };
    assert!(matches!(ingest_source(&mut store8, &s8).await.unwrap(), IngestOutcome::Inserted { .. }));
    let j8 = claim_next_job(&mut store8).await.unwrap().expect("j8");
    assert_eq!(
        commit_enrichment(
            &mut store8,
            &j8,
            &EnrichmentOutput {
                assertions: vec![AssertionInput {
                    assertion_id: "a8".into(),
                    kind: AssertionKind::Fact,
                    owner_id: "alice".into(),
                    statement: "Eight-dim vector fact".into(),
                    valid_from: now,
                    valid_to: None,
                    confidence: 1.0,
                    embedding: Some(vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
                }],
                edges: vec![],
            },
        )
        .await
        .unwrap(),
        EnrichCommit::Committed
    );
    drop(store8);
    let _ = server.batch_execute(&format!("DROP DATABASE IF EXISTS {custom_name} WITH (FORCE)")).await;
    let _ = server.batch_execute(&format!("DROP ROLE IF EXISTS {custom_name}_reader; DROP ROLE IF EXISTS {custom_name}")).await;

    // 2. record_access increments access_count and protects accessed untrusted assertions from cold archival.
    let mut store = db.store().await;
    let tenant = format!("tw_{}", Uuid::new_v4());
    let s_u = SourceInput {
        tenant_id: tenant.clone(),
        source_id: "su".into(),
        rev: 1,
        owner_id: "alice".into(),
        readers: vec![vec!["user:alice".into()]],
        label: TaintLabel { untrusted: true, private: false },
        text: "Useful untrusted note".into(),
        occurred_at: now - Duration::days(45),
    };
    assert!(matches!(ingest_source(&mut store, &s_u).await.unwrap(), IngestOutcome::Inserted { .. }));
    let ju = claim_next_job(&mut store).await.unwrap().expect("ju");
    assert_eq!(
        commit_enrichment(
            &mut store,
            &ju,
            &EnrichmentOutput {
                assertions: vec![AssertionInput {
                    assertion_id: "a_accessed".into(),
                    kind: AssertionKind::Fact,
                    owner_id: "alice".into(),
                    statement: "Useful untrusted note".into(),
                    valid_from: now - Duration::days(45),
                    valid_to: None,
                    confidence: 0.8,
                    embedding: None,
                }],
                edges: vec![],
            },
        )
        .await
        .unwrap(),
        EnrichCommit::Committed
    );
    store
        .client()
        .execute("UPDATE public.memory_assertions SET created_at = now() - interval '45 days' WHERE tenant_id = $1 AND assertion_id = 'a_accessed'", &[&tenant])
        .await
        .unwrap();
    assert_eq!(store.record_access(&tenant, &["a_accessed".into()]).await.unwrap(), 1);
    let arch = archive_cold_and_sweep_orphans(&mut store, &tenant, 30).await.unwrap();
    assert_eq!(arch.archived_untrusted, 0, "accessed untrusted assertion must not be cold-archived");

    // 3. Supersession window coverage: newer fact starting mid-window does NOT hide older fact
    //    for a window covering the earlier period, but DOES hide it when covering the full window.
    let s_hist = SourceInput {
        tenant_id: tenant.clone(),
        source_id: "sh".into(),
        rev: 1,
        owner_id: "alice".into(),
        readers: vec![vec!["user:alice".into()]],
        label: TaintLabel { untrusted: false, private: false },
        text: "VP of Engineering timeline".into(),
        occurred_at: now - Duration::days(30),
    };
    assert!(matches!(ingest_source(&mut store, &s_hist).await.unwrap(), IngestOutcome::Inserted { .. }));
    let jh = claim_next_job(&mut store).await.unwrap().expect("jh");
    assert_eq!(
        commit_enrichment(
            &mut store,
            &jh,
            &EnrichmentOutput {
                assertions: vec![
                    AssertionInput {
                        assertion_id: "a_vp_old".into(),
                        kind: AssertionKind::Fact,
                        owner_id: "alice".into(),
                        statement: "Alice is VP of Engineering".into(),
                        valid_from: now - Duration::days(30),
                        valid_to: None,
                        confidence: 1.0,
                        embedding: None,
                    },
                    AssertionInput {
                        assertion_id: "a_vp_new".into(),
                        kind: AssertionKind::Fact,
                        owner_id: "alice".into(),
                        statement: "Bob is VP of Engineering".into(),
                        valid_from: now - Duration::days(10),
                        valid_to: None,
                        confidence: 1.0,
                        embedding: None,
                    },
                ],
                edges: vec![],
            },
        )
        .await
        .unwrap(),
        EnrichCommit::Committed
    );
    store.record_supersession(&tenant, "a_vp_new", "a_vp_old").await.unwrap();

    // Window [T-20d, T-5d]: newer starts at T-10d (mid-window), so older MUST remain visible.
    let mut mid_req = RetrieveRequest::new(&tenant, vec!["user:alice".into()], "VP Engineering");
    mid_req.time_range = Some((now - Duration::days(20), now - Duration::days(5)));
    let mid_res = retrieve(&mut store, &mid_req).await.unwrap();
    let mid_ids: Vec<&str> = mid_res.hits.iter().map(|h| h.id.as_str()).collect();
    assert!(mid_ids.contains(&"a_vp_old"), "mid-window supersession must not hide older fact for earlier part of window: {mid_ids:?}");
    assert!(mid_ids.contains(&"a_vp_new"));

    // Window [T-8d, T-2d]: newer covers the entire window, so older MUST be hidden.
    let mut full_req = RetrieveRequest::new(&tenant, vec!["user:alice".into()], "VP Engineering");
    full_req.time_range = Some((now - Duration::days(8), now - Duration::days(2)));
    let full_res = retrieve(&mut store, &full_req).await.unwrap();
    let full_ids: Vec<&str> = full_res.hits.iter().map(|h| h.id.as_str()).collect();
    assert!(!full_ids.contains(&"a_vp_old"), "full-window supersession must hide older fact: {full_ids:?}");
    assert!(full_ids.contains(&"a_vp_new"));
}

/// `log::migrate` is the event log's migration; it must not create the
/// unified memory tables (they come only from `migrate_if_enabled`, which
/// checks the mode, pgvector and the configured embedding size).
#[tokio::test(flavor = "multi_thread")]
async fn log_migrate_does_not_create_unified_memory_tables() {
    let db = LiveDb::create_unmigrated().await;
    crate::log::migrate(&db.admin_url, &db.kernel_url, None).await.expect("log migration");
    let admin = crate::log::connect(&db.admin_url).await.unwrap();
    let present: bool = admin.query_one("SELECT to_regclass('public.memory_assertions') IS NOT NULL", &[]).await.unwrap().get(0);
    assert!(!present, "log::migrate must leave the unified memory tables to migrate_if_enabled");
}

/// A profile assertion is budgeted against the owner of the source it came
/// from: one person's source cannot write into another person's profile.
#[tokio::test(flavor = "multi_thread")]
async fn enrichment_refuses_a_profile_owned_by_someone_other_than_the_source_owner() {
    let db = LiveDb::create().await;
    let mut store = db.store().await;
    let tenant = format!("t_{}", Uuid::new_v4());
    let now = Utc::now();
    let src = SourceInput {
        tenant_id: tenant.clone(),
        source_id: "bob_mail".into(),
        rev: 1,
        owner_id: "bob".into(),
        readers: vec![vec!["user:bob".into()]],
        label: TaintLabel { untrusted: false, private: true },
        text: "Bob says Alice prefers tea.".into(),
        occurred_at: now,
    };
    assert!(matches!(ingest_source(&mut store, &src).await.unwrap(), IngestOutcome::Inserted { .. }));
    let job = claim_next_job(&mut store).await.unwrap().expect("job");
    let out = EnrichmentOutput {
        assertions: vec![AssertionInput {
            assertion_id: "p_alice_from_bob".into(),
            kind: AssertionKind::Profile,
            owner_id: "alice".into(),
            statement: "Prefers tea".into(),
            valid_from: now,
            valid_to: None,
            confidence: 1.0,
            embedding: None,
        }],
        edges: vec![],
    };
    let err = commit_enrichment(&mut store, &job, &out).await.expect_err("a profile for another owner is refused");
    assert!(format!("{err:#}").contains("must match source owner"), "{err:#}");
    let written: i64 = store.client().query_one("SELECT count(*) FROM public.memory_assertions WHERE tenant_id = $1", &[&tenant]).await.unwrap().get(0);
    assert_eq!(written, 0);
}

/// The profile byte budget is checked under the owner's scope lock, so a
/// concurrent profile write for the same owner (here: a held lock) is waited
/// for instead of both passing the budget check.
#[tokio::test(flavor = "multi_thread")]
async fn enrichment_profile_budget_waits_for_the_owner_scope_lock() {
    let db = LiveDb::create().await;
    let mut store = db.store().await;
    let tenant = format!("t_{}", Uuid::new_v4());
    let now = Utc::now();
    let src = SourceInput {
        tenant_id: tenant.clone(),
        source_id: "alice_profile_src".into(),
        rev: 1,
        owner_id: "alice".into(),
        readers: vec![vec!["user:alice".into()]],
        label: TaintLabel { untrusted: false, private: true },
        text: "Alice works from Lisbon.".into(),
        occurred_at: now,
    };
    assert!(matches!(ingest_source(&mut store, &src).await.unwrap(), IngestOutcome::Inserted { .. }));
    let job = claim_next_job(&mut store).await.unwrap().expect("job");
    let out = EnrichmentOutput {
        assertions: vec![AssertionInput {
            assertion_id: "p_alice_city".into(),
            kind: AssertionKind::Profile,
            owner_id: "alice".into(),
            statement: "Works from Lisbon".into(),
            valid_from: now,
            valid_to: None,
            confidence: 1.0,
            embedding: None,
        }],
        edges: vec![],
    };

    let mut holder = db.store().await;
    let held = holder.client_mut().transaction().await.unwrap();
    crate::memory::curator::lock_owner_scope(&held, &tenant, "alice").await.unwrap();

    let mut writer = db.store().await;
    let commit = tokio::spawn(async move { commit_enrichment(&mut writer, &job, &out).await });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(!commit.is_finished(), "a profile write must wait for the owner's scope lock");

    held.commit().await.unwrap();
    assert_eq!(commit.await.unwrap().unwrap(), EnrichCommit::Committed);
}
