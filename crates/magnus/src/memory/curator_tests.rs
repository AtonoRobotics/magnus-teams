//! Phase 3 curator tests: pure guards, and live Postgres tests on a fresh
//! database per test (`live_pg_test::LiveDb`).

use crate::memory::curator::{
    archive_cold_and_sweep_orphans, distill_assertion, lock_owner_scope, review_collisions, CollisionDecision, CollisionPair, CollisionReviewer, CollisionStats, DistillInput,
    DistillOutcome,
};
use crate::memory::enrich::{claim_next_job, commit_enrichment, EnrichCommit, EnrichmentOutput};
use crate::memory::ingest::{ingest_source, IngestOutcome};
use crate::memory::live_pg_test::{test_config, LiveDb};
use crate::memory::store::{MemoryStore, SupersessionOutcome};
use crate::memory::{readers_cover, AssertionInput, AssertionKind, SourceInput, SourceRef, TaintLabel};
use crate::memory_index::{MemoryConfig, MAX_LESSONS_PER_SCOPE_LIMIT, TIER2_COLD_DAYS};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::sync::atomic::{AtomicUsize, Ordering};

/// How long the profile-budget test waits to see a distill blocked on the owner lock.
const OWNER_LOCK_WAIT: std::time::Duration = std::time::Duration::from_millis(400);

const CLEAN: TaintLabel = TaintLabel { untrusted: false, private: false };
const UNTRUSTED: TaintLabel = TaintLabel { untrusted: true, private: false };
const PRIVATE: TaintLabel = TaintLabel { untrusted: false, private: true };

struct SpyReviewer {
    calls: AtomicUsize,
    decision: CollisionDecision,
}

impl SpyReviewer {
    fn new(decision: CollisionDecision) -> Self {
        Self { calls: AtomicUsize::new(0), decision }
    }

    fn count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl CollisionReviewer for SpyReviewer {
    async fn review(&self, _pair: &CollisionPair) -> anyhow::Result<CollisionDecision> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.decision)
    }
}

fn readers(principal: &str) -> Vec<Vec<String>> {
    vec![vec![principal.to_string()]]
}

fn fact(id: &str, emb: [f32; 4], valid_from: DateTime<Utc>) -> AssertionInput {
    AssertionInput {
        assertion_id: id.into(),
        kind: AssertionKind::Fact,
        owner_id: "alice".into(),
        statement: format!("statement {id}"),
        valid_from,
        valid_to: None,
        confidence: 0.9,
        embedding: Some(emb.to_vec()),
    }
}

/// Ingest one source and commit `assertions` from it through the real enrichment path.
async fn seed(store: &mut MemoryStore, tenant: &str, source_id: &str, owner: &str, rdrs: Vec<Vec<String>>, label: TaintLabel, assertions: Vec<AssertionInput>) {
    let src = SourceInput {
        tenant_id: tenant.into(),
        source_id: source_id.into(),
        rev: 1,
        owner_id: owner.into(),
        readers: rdrs,
        label,
        text: format!("source text {source_id}"),
        occurred_at: Utc::now(),
    };
    assert!(matches!(ingest_source(store, &src).await.unwrap(), IngestOutcome::Inserted { .. }));
    let job = claim_next_job(store).await.unwrap().expect("enrichment job for the seeded source");
    assert_eq!(commit_enrichment(store, &job, &EnrichmentOutput { assertions, edges: vec![] }).await.unwrap(), EnrichCommit::Committed);
}

/// Age freshly written assertions past `COLLISION_WATERMARK_SETTLE_SECS`, keeping their order.
async fn settle(store: &MemoryStore, tenant: &str) {
    store
        .client()
        .execute(
            "UPDATE public.memory_assertions SET created_at = created_at - interval '1 hour'
             WHERE tenant_id = $1 AND created_at > now() - interval '10 minutes'",
            &[&tenant],
        )
        .await
        .unwrap();
}

async fn pass(store: &mut MemoryStore, tenant: &str, decision: CollisionDecision) -> (CollisionStats, usize) {
    let spy = SpyReviewer::new(decision);
    let stats = review_collisions(store, tenant, &spy).await.unwrap();
    (stats, spy.count())
}

async fn valid_to(store: &MemoryStore, tenant: &str, id: &str) -> Option<DateTime<Utc>> {
    store.client().query_one("SELECT valid_to FROM public.memory_assertions WHERE tenant_id = $1 AND assertion_id = $2", &[&tenant, &id]).await.unwrap().get(0)
}

async fn count(store: &MemoryStore, sql: &str, tenant: &str) -> i64 {
    store.client().query_one(sql, &[&tenant]).await.unwrap().get(0)
}

#[test]
fn readers_cover_requires_every_reader_of_the_narrower_policy() {
    let org = readers("org:acme");
    let alice = readers("user:alice");
    assert!(readers_cover(&org, &org));
    assert!(!readers_cover(&alice, &org), "alice-only cannot cover every org reader");
    assert!(!readers_cover(&org, &alice), "an org policy does not cover alice unless alice is in the clause");
    assert!(readers_cover(&[vec!["org:acme".into(), "user:alice".into()]], &alice));
    assert!(readers_cover(&org, &[vec!["org:acme".into()], vec!["user:alice".into()]]), "a stricter (two-clause) policy is covered by one of its clauses");
    assert!(readers_cover(&[], &alice), "no clause admits every reader");
}

#[test]
fn assertion_kind_round_trips_through_its_column_text() {
    for k in [AssertionKind::Fact, AssertionKind::Chunk, AssertionKind::Lesson, AssertionKind::Profile] {
        assert_eq!(k.as_str().parse::<AssertionKind>().unwrap(), k);
    }
    assert!("memo".parse::<AssertionKind>().is_err());
}

#[test]
fn max_lessons_per_scope_is_validated_at_startup() {
    let mut cfg = MemoryConfig::default();
    assert!(cfg.check().is_ok());
    cfg.max_lessons_per_scope = 0;
    assert!(cfg.check().is_err());
    cfg.max_lessons_per_scope = MAX_LESSONS_PER_SCOPE_LIMIT + 1;
    assert!(cfg.check().is_err());
    let parsed: MemoryConfig = toml::from_str("max_lessons_per_scope = 7").unwrap();
    assert_eq!(parsed.max_lessons_per_scope, 7);
}

#[tokio::test(flavor = "multi_thread")]
async fn collision_review_is_idle_free_watermarked_and_merges_within_one_class() {
    let db = LiveDb::create().await;
    let mut store = db.store().await;
    let t = "t_watermark";
    let now = Utc::now();

    let (idle, calls) = pass(&mut store, t, CollisionDecision::KeepBoth).await;
    assert_eq!((idle.candidates_scanned, calls), (0, 0), "an empty tenant costs no reviewer calls");

    seed(&mut store, t, "s1", "alice", readers("user:alice"), CLEAN, vec![fact("a_dup_1", [0.99, 0.01, 0.0, 0.0], now)]).await;
    seed(&mut store, t, "s2", "alice", readers("user:alice"), CLEAN, vec![fact("a_dup_2", [0.98, 0.02, 0.0, 0.0], now)]).await;
    seed(&mut store, t, "s3", "alice", readers("user:alice"), UNTRUSTED, vec![fact("a_rumor", [0.97, 0.03, 0.0, 0.0], now)]).await;

    let (unsettled, calls) = pass(&mut store, t, CollisionDecision::MergeDuplicate).await;
    assert_eq!((unsettled.candidates_scanned, calls), (0, 0), "assertions inside the settle window wait for a later pass");

    settle(&store, t).await;
    let (merged, _) = pass(&mut store, t, CollisionDecision::MergeDuplicate).await;
    assert_eq!(merged.candidates_scanned, 3);
    assert_eq!(merged.merged, 1, "the clean duplicate merges into the older clean assertion");
    assert_eq!(merged.skipped_by_guard, 1, "the untrusted assertion is in another class and never merges");
    assert_eq!(count(&store, "SELECT count(*) FROM public.memory_evidence WHERE tenant_id = $1 AND target_id = 'a_dup_1'", t).await, 2, "merge keeps both evidence rows");
    assert_eq!(count(&store, "SELECT count(*) FROM public.memory_assertions WHERE tenant_id = $1 AND assertion_id = 'a_dup_2'", t).await, 0);

    let (again, calls) = pass(&mut store, t, CollisionDecision::MergeDuplicate).await;
    assert_eq!((again.candidates_scanned, calls), (0, 0), "a repeat pass after the watermark makes 0 reviewer calls");

    seed(&mut store, t, "s4", "alice", readers("user:alice"), CLEAN, vec![fact("a_later", [0.96, 0.04, 0.0, 0.0], now)]).await;
    settle(&store, t).await;
    let (fresh, calls) = pass(&mut store, t, CollisionDecision::KeepBoth).await;
    assert_eq!(fresh.candidates_scanned, 1, "only the assertion written since the watermark is scanned");
    assert!(calls >= 1 && fresh.kept_both == calls);
}

#[tokio::test(flavor = "multi_thread")]
async fn merge_moves_supersession_links_onto_the_kept_assertion() {
    let db = LiveDb::create().await;
    let mut store = db.store().await;
    let t = "t_merge_links";
    let now = Utc::now();

    seed(&mut store, t, "s_old", "alice", readers("org:acme"), CLEAN, vec![fact("a_old", [0.0, 1.0, 0.0, 0.0], now)]).await;
    seed(&mut store, t, "s_x", "alice", readers("org:acme"), CLEAN, vec![fact("a_x", [1.0, 0.0, 0.0, 0.0], now)]).await;
    seed(&mut store, t, "s_y", "alice", readers("org:acme"), CLEAN, vec![fact("a_y", [0.99, 0.01, 0.0, 0.0], now)]).await;
    assert_eq!(store.record_supersession(t, "a_y", "a_old").await.unwrap(), SupersessionOutcome::Recorded);
    settle(&store, t).await;

    let (stats, _) = pass(&mut store, t, CollisionDecision::MergeDuplicate).await;
    assert_eq!(stats.merged, 1);
    assert_eq!(
        count(&store, "SELECT count(*) FROM public.memory_supersessions WHERE tenant_id = $1 AND new_assertion_id = 'a_x' AND old_assertion_id = 'a_old'", t).await,
        1,
        "the duplicate's supersession of a_old must survive on the kept assertion"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn temporal_update_needs_reader_coverage_later_start_and_trust() {
    let db = LiveDb::create().await;
    let mut store = db.store().await;
    let t = "t_temporal";
    let now = Utc::now();

    seed(&mut store, t, "s_team", "alice", readers("org:acme"), CLEAN, vec![fact("a_team", [1.0, 0.0, 0.0, 0.0], now - Duration::days(10))]).await;
    seed(&mut store, t, "s_mine", "alice", readers("user:alice"), PRIVATE, vec![fact("a_mine", [0.99, 0.01, 0.0, 0.0], now)]).await;
    settle(&store, t).await;
    let (s1, _) = pass(&mut store, t, CollisionDecision::TemporalUpdate).await;
    assert_eq!(s1.temporal_closed, 0, "a private fact alice alone reads must not end the team's fact for everyone");
    assert!(valid_to(&store, t, "a_team").await.is_none());

    seed(&mut store, t, "s_back", "alice", readers("org:acme"), CLEAN, vec![fact("a_backdated", [0.98, 0.02, 0.0, 0.0], now - Duration::days(20))]).await;
    settle(&store, t).await;
    let (s2, _) = pass(&mut store, t, CollisionDecision::TemporalUpdate).await;
    assert_eq!(s2.temporal_closed, 0, "a fact starting before the older one cannot close it");
    assert!(valid_to(&store, t, "a_team").await.is_none());

    let ok_from = now - Duration::days(1);
    seed(&mut store, t, "s_ok", "alice", readers("org:acme"), CLEAN, vec![fact("a_ok", [0.97, 0.03, 0.0, 0.0], ok_from)]).await;
    settle(&store, t).await;
    let (s3, _) = pass(&mut store, t, CollisionDecision::TemporalUpdate).await;
    assert!(s3.temporal_closed >= 1);
    let closed = valid_to(&store, t, "a_team").await.expect("the team fact is closed by a later fact every team reader can see");
    assert_eq!(closed.timestamp_micros(), ok_from.timestamp_micros());

    seed(&mut store, t, "s_rumor", "alice", readers("org:acme"), UNTRUSTED, vec![fact("a_rumor", [0.96, 0.04, 0.0, 0.0], now)]).await;
    settle(&store, t).await;
    let (s4, _) = pass(&mut store, t, CollisionDecision::TemporalUpdate).await;
    assert_eq!(s4.temporal_closed, 0, "untrusted content never closes a trusted fact (Invariant 4)");
    assert!(valid_to(&store, t, "a_ok").await.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn supersede_counts_policy_refusals_as_guard_skips_not_errors() {
    let db = LiveDb::create().await;
    let mut store = db.store().await;
    let t = "t_supersede";
    let now = Utc::now();

    seed(&mut store, t, "s_t", "alice", readers("org:acme"), CLEAN, vec![fact("a_trusted", [1.0, 0.0, 0.0, 0.0], now)]).await;
    seed(&mut store, t, "s_u", "alice", readers("org:acme"), UNTRUSTED, vec![fact("a_untrusted", [0.99, 0.01, 0.0, 0.0], now)]).await;
    settle(&store, t).await;
    let (s1, _) = pass(&mut store, t, CollisionDecision::Supersede).await;
    assert_eq!((s1.superseded, s1.skipped_by_guard), (0, 1), "the untrusted override is refused and counted, and the pass still succeeds");

    seed(&mut store, t, "s_t2", "alice", readers("org:acme"), CLEAN, vec![fact("a_trusted_2", [0.98, 0.02, 0.0, 0.0], now)]).await;
    settle(&store, t).await;
    let (s2, _) = pass(&mut store, t, CollisionDecision::Supersede).await;
    assert!(s2.superseded >= 1);
    assert_eq!(
        count(&store, "SELECT count(*) FROM public.memory_supersessions WHERE tenant_id = $1 AND new_assertion_id = 'a_trusted_2' AND old_assertion_id = 'a_trusted'", t).await,
        1
    );
}

fn distill_input(t: &str, id: &str, kind: AssertionKind, statement: String, sources: &[&str]) -> DistillInput {
    DistillInput {
        tenant_id: t.into(),
        assertion_id: id.into(),
        kind,
        owner_id: "alice".into(),
        statement,
        valid_from: Utc::now(),
        confidence: 0.9,
        embedding: None,
        sources: sources.iter().map(|s| SourceRef { source_id: (*s).into(), rev: 1 }).collect(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn distillation_inherits_strictest_readers_and_taint_and_stays_in_one_owner_scope() {
    let db = LiveDb::create().await;
    let mut store = db.store().await;
    let cfg = test_config();
    let t = "t_distill";

    seed(&mut store, t, "s_team", "alice", readers("org:acme"), CLEAN, vec![]).await;
    seed(&mut store, t, "s_priv", "alice", readers("user:alice"), PRIVATE, vec![]).await;
    seed(&mut store, t, "s_web", "alice", readers("org:acme"), UNTRUSTED, vec![]).await;
    seed(&mut store, t, "s_bob", "bob", readers("org:acme"), CLEAN, vec![]).await;

    let lesson = distill_input(t, "les_mixed", AssertionKind::Lesson, "Mixed lesson".into(), &["s_team", "s_priv"]);
    assert_eq!(distill_assertion(&mut store, &lesson, &cfg).await.unwrap(), DistillOutcome::Committed { archived_excess: 0 });
    let row =
        store.client().query_one("SELECT readers, untrusted, private FROM public.memory_assertions WHERE tenant_id = $1 AND assertion_id = 'les_mixed'", &[&t]).await.unwrap();
    let got: Vec<Vec<String>> = serde_json::from_value(row.get(0)).unwrap();
    assert_eq!(got, vec![vec!["org:acme".to_string()], vec!["user:alice".to_string()]], "readers are the intersection of both sources");
    assert_eq!((row.get::<_, bool>(1), row.get::<_, bool>(2)), (false, true), "private is carried, untrusted stays clear");
    assert_eq!(count(&store, "SELECT count(*) FROM public.memory_evidence WHERE tenant_id = $1 AND target_id = 'les_mixed'", t).await, 2);

    let cross = distill_input(t, "les_cross", AssertionKind::Lesson, "Cross-owner lesson".into(), &["s_team", "s_bob"]);
    assert!(distill_assertion(&mut store, &cross, &cfg).await.is_err(), "bob's source cannot feed alice's lesson");
    let tainted = distill_input(t, "prof_tainted", AssertionKind::Profile, "Injected rule".into(), &["s_team", "s_web"]);
    assert!(distill_assertion(&mut store, &tainted, &cfg).await.is_err(), "untrusted input never becomes a profile entry");
    assert_eq!(count(&store, "SELECT count(*) FROM public.memory_assertions WHERE tenant_id = $1 AND assertion_id IN ('les_cross', 'prof_tainted')", t).await, 0);

    seed(&mut store, t, "s_gone", "alice", readers("org:acme"), CLEAN, vec![]).await;
    assert_eq!(store.hard_delete_source_rev(t, "s_gone", 1).await.unwrap(), 1);
    let fenced = distill_input(t, "les_fenced", AssertionKind::Lesson, "From an erased source".into(), &["s_team", "s_gone"]);
    assert_eq!(distill_assertion(&mut store, &fenced, &cfg).await.unwrap(), DistillOutcome::FencedDeleted);
}

#[tokio::test(flavor = "multi_thread")]
async fn lesson_cap_archives_the_least_used_others_never_the_new_lesson() {
    let db = LiveDb::create().await;
    let mut store = db.store().await;
    let cfg = MemoryConfig { max_lessons_per_scope: 2, ..test_config() };
    let t = "t_cap";
    seed(&mut store, t, "s_team", "alice", readers("org:acme"), CLEAN, vec![]).await;

    for id in ["les_1", "les_2"] {
        let input = distill_input(t, id, AssertionKind::Lesson, format!("Lesson {id}"), &["s_team"]);
        assert_eq!(distill_assertion(&mut store, &input, &cfg).await.unwrap(), DistillOutcome::Committed { archived_excess: 0 });
    }
    store.client().execute("UPDATE public.memory_assertions SET access_count = 5 WHERE tenant_id = $1", &[&t]).await.unwrap();

    let newest = distill_input(t, "les_3", AssertionKind::Lesson, "Lesson les_3".into(), &["s_team"]);
    assert_eq!(distill_assertion(&mut store, &newest, &cfg).await.unwrap(), DistillOutcome::Committed { archived_excess: 1 });
    let active: bool = store.client().query_one("SELECT NOT archived FROM public.memory_assertions WHERE tenant_id = $1 AND assertion_id = 'les_3'", &[&t]).await.unwrap().get(0);
    assert!(active, "the lesson just distilled must not be the one archived");
    assert_eq!(count(&store, "SELECT count(*) FROM public.memory_assertions WHERE tenant_id = $1 AND kind = 'lesson' AND NOT archived", t).await, 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_budget_holds_under_concurrent_distills_for_one_owner() {
    let db = LiveDb::create().await;
    let mut store = db.store().await;
    let cfg = test_config();
    let t = "t_budget";
    seed(&mut store, t, "s_me", "alice", readers("user:alice"), CLEAN, vec![]).await;

    // A concurrent writer holds alice's owner lock and has an uncommitted 5,000-byte profile entry.
    let mut holder = db.store().await;
    let tx = holder.client_mut().transaction().await.unwrap();
    lock_owner_scope(&tx, t, "alice").await.unwrap();
    let held = "h".repeat(5000);
    tx.execute(
        "INSERT INTO public.memory_assertions (tenant_id, assertion_id, kind, owner_id, readers, statement, valid_from, tsv)
         VALUES ($1, 'prof_held', 'profile', 'alice', '[[\"user:alice\"]]'::jsonb, $2, now(), to_tsvector('simple', $2))",
        &[&t, &held],
    )
    .await
    .unwrap();

    let input = distill_input(t, "prof_racing", AssertionKind::Profile, "r".repeat(5000), &["s_me"]);
    let racing = tokio::spawn(async move { distill_assertion(&mut store, &input, &cfg).await });
    tokio::time::sleep(OWNER_LOCK_WAIT).await;
    assert!(!racing.is_finished(), "a second distill for the same owner waits for the first to finish");

    tx.commit().await.unwrap();
    let result = racing.await.unwrap();
    assert!(result.is_err(), "after the first commits, 5,000 + 5,000 bytes exceeds the 8 KB profile budget");
}

#[tokio::test(flavor = "multi_thread")]
async fn cold_archival_only_touches_unused_untrusted_assertions_and_orphan_entities() {
    let db = LiveDb::create().await;
    let mut store = db.store().await;
    let t = "t_cold";
    let now = Utc::now();

    seed(&mut store, t, "s_clean", "alice", readers("org:acme"), CLEAN, vec![fact("a_clean", [1.0, 0.0, 0.0, 0.0], now)]).await;
    seed(&mut store, t, "s_web", "alice", readers("org:acme"), UNTRUSTED, vec![fact("a_web", [0.0, 1.0, 0.0, 0.0], now)]).await;
    store.client().execute("UPDATE public.memory_assertions SET created_at = now() - interval '45 days' WHERE tenant_id = $1", &[&t]).await.unwrap();
    store.client().execute("INSERT INTO public.memory_entities (tenant_id, entity_id, entity_type) VALUES ($1, 'ent_orphan', 'misc')", &[&t]).await.unwrap();

    let arch = archive_cold_and_sweep_orphans(&mut store, t, TIER2_COLD_DAYS).await.unwrap();
    assert_eq!(arch.archived_untrusted, 1, "only the cold unused untrusted assertion is archived");
    assert_eq!(arch.swept_orphan_entities, 1);
    let clean_active: bool =
        store.client().query_one("SELECT NOT archived FROM public.memory_assertions WHERE tenant_id = $1 AND assertion_id = 'a_clean'", &[&t]).await.unwrap().get(0);
    assert!(clean_active, "trusted assertions are never cold-archived");
}
