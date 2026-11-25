//! 3-job gated background memory curator (0 LLM calls when idle):
//! 1. Watermarked high-cosine collision review (merge / temporal close / supersession)
//! 2. Fenced lesson & profile distillation with strict taint/ACL inheritance & cap
//! 3. Cold untrusted archival + orphan entity sweep (0 LLM)

use crate::memory::store::{supersession_reaches, MemoryStore, SupersessionOutcome};
use crate::memory::{canonicalize_readers, format_halfvec_literal, intersect_readers, readers_cover, AssertionKind, SourceRef, TaintLabel};
use crate::memory_index::{MemoryConfig, PROFILE_MAX_BYTES};
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use tokio_postgres::{Row, Transaction};

/// Maximum cosine distance (`1.0 - cosine_similarity`) that triggers collision review (0.15 == sim >= 0.85).
pub const COLLISION_MAX_COSINE_DISTANCE: f64 = 0.15;
/// Maximum newer assertions examined per collision pass (bounds reviewer calls per pass).
pub const COLLISION_BATCH_LIMIT: i64 = 32;
/// Maximum older neighbours reviewed against each newer assertion.
pub const COLLISION_NEIGHBOURS_PER_ASSERTION: i64 = 4;
/// Assertions younger than this are left for a later pass, so a write whose
/// transaction began before the watermark moved past it is never skipped.
pub const COLLISION_WATERMARK_SETTLE_SECS: i64 = 300;
/// `memory_curator_watermarks.job` for collision review.
const COLLISION_JOB: &str = "collision_review";

/// Snapshot of an active assertion participating in a high-cosine collision.
#[derive(Clone, Debug)]
pub struct CollisionAssertion {
    pub assertion_id: String,
    pub kind: AssertionKind,
    pub owner_id: String,
    pub readers: Vec<Vec<String>>,
    pub label: TaintLabel,
    pub statement: String,
    pub valid_from: DateTime<Utc>,
    pub valid_to: Option<DateTime<Utc>>,
}

impl CollisionAssertion {
    /// Decode the nine assertion columns starting at `base`:
    /// `assertion_id, kind, owner_id, readers, untrusted, private, statement, valid_from, valid_to`.
    fn from_row(row: &Row, base: usize) -> Result<Self> {
        let readers: Vec<Vec<String>> = serde_json::from_value(row.get::<_, Value>(base + 3)).context("decode assertion readers")?;
        Ok(Self {
            assertion_id: row.get(base),
            kind: row.get::<_, String>(base + 1).parse()?,
            owner_id: row.get(base + 2),
            readers: canonicalize_readers(&readers),
            label: TaintLabel { untrusted: row.get(base + 4), private: row.get(base + 5) },
            statement: row.get(base + 6),
            valid_from: row.get(base + 7),
            valid_to: row.get(base + 8),
        })
    }
}

/// Pair of high-similarity assertions `(newer, older)` handed to a `CollisionReviewer`.
#[derive(Clone, Debug)]
pub struct CollisionPair {
    pub newer: CollisionAssertion,
    pub older: CollisionAssertion,
    pub cosine_distance: f64,
}

/// Classification decision returned by a `CollisionReviewer` (LLM or deterministic test double).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollisionDecision {
    /// Identical meaning within the same `(kind, owner, readers, label)` class: merge into `older`.
    MergeDuplicate,
    /// State changed over time: close `older.valid_to = newer.valid_from`.
    TemporalUpdate,
    /// Factual correction: insert `(newer -> older)` into `memory_supersessions`.
    Supersede,
    /// Distinct facts or lower-trust claim: leave both assertions untouched.
    KeepBoth,
}

/// Pluggable reviewer for high-cosine assertion collisions (0 calls when no collisions exist).
#[async_trait]
pub trait CollisionReviewer: Send + Sync {
    async fn review(&self, pair: &CollisionPair) -> Result<CollisionDecision>;
}

/// Summary counters returned by one collision-review curator pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CollisionStats {
    pub candidates_scanned: usize,
    pub pairs_reviewed: usize,
    pub merged: usize,
    pub temporal_closed: usize,
    pub superseded: usize,
    pub kept_both: usize,
    pub skipped_by_guard: usize,
}

/// Input for fenced distillation of a multi-source `lesson` or `profile` assertion.
#[derive(Clone, Debug)]
pub struct DistillInput {
    pub tenant_id: String,
    pub assertion_id: String,
    pub kind: AssertionKind,
    pub owner_id: String,
    pub statement: String,
    pub valid_from: DateTime<Utc>,
    pub confidence: f32,
    pub embedding: Option<Vec<f32>>,
    pub sources: Vec<SourceRef>,
}

/// Outcome of `distill_assertion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistillOutcome {
    /// Distilled assertion committed with `archived_excess` other lessons archived to enforce the cap.
    Committed { archived_excess: u64 },
    /// At least one contributing source was hard-deleted mid-flight; rolled back cleanly.
    FencedDeleted,
}

/// Summary counters returned by `archive_cold_and_sweep_orphans`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArchivalStats {
    pub archived_untrusted: u64,
    pub swept_orphan_entities: u64,
}

/// Job 1: Review active, live, unsuperseded embedded assertions written since
/// the tenant's collision watermark against their nearest older neighbours
/// within `COLLISION_MAX_COSINE_DISTANCE`. The watermark advances after each
/// candidate, so a pass with nothing new makes 0 reviewer calls and 0 writes.
pub async fn review_collisions(store: &mut MemoryStore, tenant_id: &str, reviewer: &dyn CollisionReviewer) -> Result<CollisionStats> {
    let mark = store
        .client()
        .query_opt(
            "SELECT last_created_at, last_assertion_id FROM public.memory_curator_watermarks
             WHERE tenant_id = $1 AND job = $2",
            &[&tenant_id, &COLLISION_JOB],
        )
        .await
        .context("read collision watermark")?;
    let (mark_at, mark_id): (Option<DateTime<Utc>>, Option<String>) = match mark {
        Some(r) => (Some(r.get(0)), Some(r.get(1))),
        None => (None, None),
    };

    let candidates = store
        .client()
        .query(
            "SELECT n.assertion_id, n.kind, n.owner_id, n.readers, n.untrusted, n.private,
                    n.statement, n.valid_from, n.valid_to, n.created_at
             FROM public.memory_assertions n
             WHERE n.tenant_id = $1
               AND NOT n.archived
               AND n.embedding IS NOT NULL
               AND (n.valid_to IS NULL OR n.valid_to > now())
               AND NOT EXISTS (
                   SELECT 1 FROM public.memory_supersessions s
                   WHERE s.tenant_id = n.tenant_id AND s.old_assertion_id = n.assertion_id
               )
               AND n.created_at < now() - ($2::bigint * interval '1 second')
               AND ($3::timestamptz IS NULL OR (n.created_at, n.assertion_id) > ($3::timestamptz, $4::text))
             ORDER BY n.created_at, n.assertion_id
             LIMIT $5",
            &[&tenant_id, &COLLISION_WATERMARK_SETTLE_SECS, &mark_at, &mark_id, &COLLISION_BATCH_LIMIT],
        )
        .await
        .context("scan collision candidates")?;

    let mut stats = CollisionStats::default();
    for cand in &candidates {
        stats.candidates_scanned += 1;
        let newer = CollisionAssertion::from_row(cand, 0)?;
        let created_at: DateTime<Utc> = cand.get(9);
        review_candidate(store, tenant_id, &newer, reviewer, &mut stats).await?;
        store
            .client()
            .execute(
                "INSERT INTO public.memory_curator_watermarks (tenant_id, job, last_created_at, last_assertion_id)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (tenant_id, job) DO UPDATE
                 SET last_created_at = EXCLUDED.last_created_at,
                     last_assertion_id = EXCLUDED.last_assertion_id,
                     updated_at = now()",
                &[&tenant_id, &COLLISION_JOB, &created_at, &newer.assertion_id],
            )
            .await
            .context("advance collision watermark")?;
    }
    Ok(stats)
}

async fn review_candidate(store: &mut MemoryStore, tenant_id: &str, newer: &CollisionAssertion, reviewer: &dyn CollisionReviewer, stats: &mut CollisionStats) -> Result<()> {
    let neighbours = store
        .client()
        .query(
            "SELECT o.assertion_id, o.kind, o.owner_id, o.readers, o.untrusted, o.private,
                    o.statement, o.valid_from, o.valid_to,
                    (o.embedding <=> n.embedding)::double precision AS dist
             FROM public.memory_assertions n
             JOIN public.memory_assertions o ON o.tenant_id = n.tenant_id
             WHERE n.tenant_id = $1
               AND n.assertion_id = $2
               AND o.assertion_id <> n.assertion_id
               AND NOT o.archived
               AND o.embedding IS NOT NULL
               AND (o.valid_to IS NULL OR o.valid_to > now())
               AND (o.created_at, o.assertion_id) < (n.created_at, n.assertion_id)
               AND NOT EXISTS (
                   SELECT 1 FROM public.memory_supersessions s
                   WHERE s.tenant_id = o.tenant_id
                     AND (s.old_assertion_id = o.assertion_id
                       OR (s.new_assertion_id = o.assertion_id AND s.old_assertion_id = n.assertion_id))
               )
               AND (o.embedding <=> n.embedding) <= $3
             ORDER BY dist ASC
             LIMIT $4",
            &[&tenant_id, &newer.assertion_id, &COLLISION_MAX_COSINE_DISTANCE, &COLLISION_NEIGHBOURS_PER_ASSERTION],
        )
        .await
        .context("query collision neighbours")?;

    for row in &neighbours {
        let pair = CollisionPair { newer: newer.clone(), older: CollisionAssertion::from_row(row, 0)?, cosine_distance: row.get(9) };
        stats.pairs_reviewed += 1;
        match reviewer.review(&pair).await? {
            CollisionDecision::MergeDuplicate => {
                if merge_duplicate_pair(store, tenant_id, &pair).await? {
                    stats.merged += 1;
                    // The candidate no longer exists; its other pairs are moot.
                    return Ok(());
                }
                stats.skipped_by_guard += 1;
            }
            CollisionDecision::TemporalUpdate => {
                if close_temporal_pair(store, tenant_id, &pair).await? {
                    stats.temporal_closed += 1;
                } else {
                    stats.skipped_by_guard += 1;
                }
            }
            CollisionDecision::Supersede => match store.record_supersession(tenant_id, &pair.newer.assertion_id, &pair.older.assertion_id).await? {
                SupersessionOutcome::Recorded => stats.superseded += 1,
                SupersessionOutcome::Refused(_) => stats.skipped_by_guard += 1,
            },
            CollisionDecision::KeepBoth => stats.kept_both += 1,
        }
    }
    Ok(())
}

/// Merge `newer` into `older`: evidence, access counts and supersession links
/// move onto `older`, then `newer` is deleted. Refused (rolled back) across
/// equivalence classes or when the moved links would form a cycle.
async fn merge_duplicate_pair(store: &mut MemoryStore, tenant_id: &str, pair: &CollisionPair) -> Result<bool> {
    let (n, o) = (&pair.newer, &pair.older);
    if n.kind != o.kind || n.owner_id != o.owner_id || n.label != o.label || n.readers != o.readers {
        return Ok(false);
    }
    let tx = store.client_mut().transaction().await.context("begin merge_duplicate_pair tx")?;

    let locked = tx
        .query(
            "SELECT assertion_id FROM public.memory_assertions
             WHERE tenant_id = $1 AND assertion_id IN ($2, $3)
             FOR UPDATE",
            &[&tenant_id, &n.assertion_id, &o.assertion_id],
        )
        .await?;
    if locked.len() != 2 {
        tx.rollback().await?;
        return Ok(false);
    }

    let moves: [&str; 4] = [
        "INSERT INTO public.memory_evidence (tenant_id, target_kind, target_id, source_id, source_rev)
         SELECT tenant_id, 'assertion', $3, source_id, source_rev
         FROM public.memory_evidence
         WHERE tenant_id = $1 AND target_kind = 'assertion' AND target_id = $2
         ON CONFLICT DO NOTHING",
        "INSERT INTO public.memory_supersessions (tenant_id, new_assertion_id, old_assertion_id)
         SELECT tenant_id, $3, old_assertion_id
         FROM public.memory_supersessions
         WHERE tenant_id = $1 AND new_assertion_id = $2 AND old_assertion_id <> $3
         ON CONFLICT DO NOTHING",
        "INSERT INTO public.memory_supersessions (tenant_id, new_assertion_id, old_assertion_id)
         SELECT tenant_id, new_assertion_id, $3
         FROM public.memory_supersessions
         WHERE tenant_id = $1 AND old_assertion_id = $2 AND new_assertion_id <> $3
         ON CONFLICT DO NOTHING",
        "UPDATE public.memory_assertions o
         SET access_count = o.access_count + n.access_count
         FROM public.memory_assertions n
         WHERE o.tenant_id = $1 AND o.assertion_id = $3
           AND n.tenant_id = $1 AND n.assertion_id = $2",
    ];
    for sql in moves {
        tx.execute(sql, &[&tenant_id, &n.assertion_id, &o.assertion_id]).await.context("move duplicate onto canonical assertion")?;
    }
    tx.execute("DELETE FROM public.memory_evidence WHERE tenant_id = $1 AND target_kind = 'assertion' AND target_id = $2", &[&tenant_id, &n.assertion_id]).await?;
    // Cascades the duplicate's own supersession links, which now live on `older`.
    tx.execute("DELETE FROM public.memory_assertions WHERE tenant_id = $1 AND assertion_id = $2", &[&tenant_id, &n.assertion_id]).await?;

    if supersession_reaches(&tx, tenant_id, &o.assertion_id, &o.assertion_id).await? {
        tx.rollback().await?;
        return Ok(false);
    }
    tx.commit().await?;
    Ok(true)
}

/// Close `older` at `newer.valid_from`. This changes what every reader of
/// `older` sees, so it is refused unless every such reader can also see
/// `newer` (readers coverage, no new `private` bit), `newer` is not an
/// untrusted override of a trusted fact (Invariant 4), a profile entry is only
/// closed by a profile entry, and `newer` actually starts after `older`.
async fn close_temporal_pair(store: &mut MemoryStore, tenant_id: &str, pair: &CollisionPair) -> Result<bool> {
    let (n, o) = (&pair.newer, &pair.older);
    let refused = (n.label.untrusted && !o.label.untrusted)
        || (n.label.private && !o.label.private)
        || !readers_cover(&n.readers, &o.readers)
        || (o.kind == AssertionKind::Profile && n.kind != AssertionKind::Profile)
        || n.valid_from <= o.valid_from;
    if refused {
        return Ok(false);
    }
    let updated = store
        .client()
        .execute(
            "UPDATE public.memory_assertions
             SET valid_to = $3
             WHERE tenant_id = $1
               AND assertion_id = $2
               AND valid_from < $3
               AND (valid_to IS NULL OR valid_to > $3)",
            &[&tenant_id, &o.assertion_id, &n.valid_from],
        )
        .await?;
    Ok(updated > 0)
}

/// Serialize writers of one `(tenant_id, owner_id)` scope for the rest of `tx`,
/// so the profile byte budget and lesson cap are checked against settled totals.
pub(crate) async fn lock_owner_scope(tx: &Transaction<'_>, tenant_id: &str, owner_id: &str) -> Result<()> {
    tx.execute("SELECT pg_advisory_xact_lock(hashtext($1), hashtext($2))", &[&tenant_id, &owner_id]).await.context("lock owner scope")?;
    Ok(())
}

/// Job 2: Fenced distillation of a `lesson` or `profile` assertion from one or
/// more contributing sources, all owned by `input.owner_id`. Serializes per
/// owner, locks sources `FOR SHARE`, takes the strictest `readers`
/// intersection and the OR of taint, enforces `PROFILE_MAX_BYTES` and
/// `cfg.max_lessons_per_scope` (never archiving the lesson just written).
pub async fn distill_assertion(store: &mut MemoryStore, input: &DistillInput, cfg: &MemoryConfig) -> Result<DistillOutcome> {
    anyhow::ensure!(!input.sources.is_empty(), "distillation requires at least one contributing source");
    anyhow::ensure!(matches!(input.kind, AssertionKind::Lesson | AssertionKind::Profile), "distillation only produces Lesson or Profile kinds");
    let cap = i64::try_from(cfg.max_lessons_per_scope).context("max_lessons_per_scope fits in i64")?;
    anyhow::ensure!(cap >= 1, "max_lessons_per_scope must be >= 1");

    let emb_lit = match &input.embedding {
        Some(v) => Some(format_halfvec_literal(v, store.embedding_dim())?),
        None => None,
    };

    let tx = store.client_mut().transaction().await.context("begin distill tx")?;
    lock_owner_scope(&tx, &input.tenant_id, &input.owner_id).await?;

    let mut reader_policies = Vec::with_capacity(input.sources.len());
    let mut joined_label = TaintLabel::default();
    for sref in &input.sources {
        let Some(row) = tx
            .query_opt(
                "SELECT readers, untrusted, private, owner_id
                 FROM public.memory_sources
                 WHERE tenant_id = $1 AND source_id = $2 AND rev = $3
                 FOR SHARE",
                &[&input.tenant_id, &sref.source_id, &sref.rev],
            )
            .await?
        else {
            tx.rollback().await?;
            return Ok(DistillOutcome::FencedDeleted);
        };
        let source_owner: String = row.get(3);
        anyhow::ensure!(source_owner == input.owner_id, "source {}@{} belongs to another owner; distillation stays within one owner's scope", sref.source_id, sref.rev);
        reader_policies.push(serde_json::from_value::<Vec<Vec<String>>>(row.get::<_, Value>(0)).context("decode source readers")?);
        joined_label = joined_label.join(TaintLabel { untrusted: row.get(1), private: row.get(2) });
    }

    if input.kind == AssertionKind::Profile {
        anyhow::ensure!(!joined_label.untrusted, "refusing to distill untrusted source into kind = 'profile'");
        let existing: i64 = tx
            .query_one(
                "SELECT COALESCE(SUM(octet_length(statement)), 0)::bigint
                 FROM public.memory_assertions
                 WHERE tenant_id = $1 AND owner_id = $2 AND kind = 'profile' AND NOT archived",
                &[&input.tenant_id, &input.owner_id],
            )
            .await?
            .get(0);
        let total = usize::try_from(existing).context("profile byte total is non-negative")?.saturating_add(input.statement.len());
        anyhow::ensure!(total <= PROFILE_MAX_BYTES, "user profile total ({total} bytes) exceeds PROFILE_MAX_BYTES ({PROFILE_MAX_BYTES} bytes)");
    }

    let merged_readers = json!(intersect_readers(&reader_policies));
    tx.execute(
        "INSERT INTO public.memory_assertions (
             tenant_id, assertion_id, kind, owner_id, readers, untrusted, private,
             statement, valid_from, confidence, tsv, embedding
         ) VALUES (
             $1, $2, $3, $4, $5, $6, $7,
             $8, $9, $10, to_tsvector('simple', $8), $11::text::halfvec
         )",
        &[
            &input.tenant_id,
            &input.assertion_id,
            &input.kind.as_str(),
            &input.owner_id,
            &merged_readers,
            &joined_label.untrusted,
            &joined_label.private,
            &input.statement,
            &input.valid_from,
            &input.confidence,
            &emb_lit,
        ],
    )
    .await
    .context("insert distilled assertion")?;

    for sref in &input.sources {
        tx.execute(
            "INSERT INTO public.memory_evidence (tenant_id, target_kind, target_id, source_id, source_rev)
             VALUES ($1, 'assertion', $2, $3, $4)
             ON CONFLICT DO NOTHING",
            &[&input.tenant_id, &input.assertion_id, &sref.source_id, &sref.rev],
        )
        .await?;
    }

    let mut archived_excess = 0u64;
    if input.kind == AssertionKind::Lesson {
        archived_excess = tx
            .execute(
                "WITH ranked AS (
                     SELECT assertion_id,
                            row_number() OVER (
                                ORDER BY (assertion_id = $4) DESC, access_count DESC, created_at DESC, assertion_id DESC
                            ) AS rn
                     FROM public.memory_assertions
                     WHERE tenant_id = $1 AND owner_id = $2 AND kind = 'lesson' AND NOT archived
                 )
                 UPDATE public.memory_assertions a
                 SET archived = true
                 FROM ranked r
                 WHERE a.tenant_id = $1 AND a.assertion_id = r.assertion_id AND r.rn > $3",
                &[&input.tenant_id, &input.owner_id, &cap, &input.assertion_id],
            )
            .await?;
    }

    tx.commit().await?;
    Ok(DistillOutcome::Committed { archived_excess })
}

/// Job 3 (0 LLM): Archive unused `untrusted` assertions older than `cold_days`
/// (callers pass `TIER2_COLD_DAYS`) and sweep orphaned `memory_entities`.
pub async fn archive_cold_and_sweep_orphans(store: &mut MemoryStore, tenant_id: &str, cold_days: i64) -> Result<ArchivalStats> {
    anyhow::ensure!(cold_days >= 1, "cold_days must be >= 1, got {cold_days}");
    let tx = store.client_mut().transaction().await.context("begin archive_cold_and_sweep_orphans tx")?;

    let archived_untrusted = tx
        .execute(
            "UPDATE public.memory_assertions
             SET archived = true
             WHERE tenant_id = $1
               AND untrusted
               AND NOT archived
               AND access_count = 0
               AND created_at < now() - ($2::bigint * interval '1 day')",
            &[&tenant_id, &cold_days],
        )
        .await
        .context("archive cold untrusted assertions")?;

    let swept_orphan_entities = tx
        .execute(
            "DELETE FROM public.memory_entities ent
             WHERE ent.tenant_id = $1
               AND NOT EXISTS (
                   SELECT 1 FROM public.memory_edges e
                   WHERE e.tenant_id = ent.tenant_id
                     AND (e.src_id = ent.entity_id OR e.dst_id = ent.entity_id)
               )",
            &[&tenant_id],
        )
        .await
        .context("sweep orphaned memory_entities")?;

    tx.commit().await?;
    Ok(ArchivalStats { archived_untrusted, swept_orphan_entities })
}
