//! 5-job watermarked background curator for the unified memory engine.
//!
//! Every stage checks its watermark (`memory_curator_state`) first so idle
//! tenants perform zero LLM calls and minimal DB work:
//! 1. Feedback weighting (`feedback.rs`, 0 LLM).
//! 2. Semantic deduplication within identical `(readers, untrusted)` classes,
//!    unioning `memory_evidence` rows so provenance is preserved.
//! 3. Incremental conflict & supersession review (`supersedes` if `new.trust >= old.trust`,
//!    otherwise `contradicts`).
//! 4. Bounded trace & session lesson distillation (failed->recovered / corrected runs,
//!    capped at `MAX_LESSONS_PER_SCOPE`, strict taint inheritance).
//! 5. Cold archival of unused, low-feedback `untrusted` assertions after `COLD_UNTRUSTED_DAYS`.

use crate::memory::{MemoryEngine, TrustLabel, COLD_UNTRUSTED_DAYS, CONFLICT_COSINE_THRESHOLD, DEDUP_COSINE_THRESHOLD, MAX_LESSONS_PER_SCOPE};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::json;

/// Summary of one curator cycle across a tenant.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CuratorReport {
    pub duplicates_merged: u64,
    pub conflicts_linked: u64,
    pub supersessions_linked: u64,
    pub lessons_pruned: u64,
    pub cold_archived: u64,
}

/// Candidate pair discovered during incremental conflict/supersession review.
#[derive(Clone, Debug)]
pub struct CollisionPair {
    pub newer_id: String,
    pub newer_statement: String,
    pub newer_label: TrustLabel,
    pub older_id: String,
    pub older_statement: String,
    pub older_label: TrustLabel,
    pub cosine_similarity: f64,
}

/// Resolution chosen for a collision pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollisionResolution {
    Supersede,
    Contradict,
    Ignore,
}

impl MemoryEngine {
    /// True if any new or updated assertions arrived since the stage's watermark.
    /// Enables zero-LLM idle gating.
    pub async fn stage_has_dirty_work(&self, tenant_id: &str, stage: &str) -> Result<bool> {
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let row = client
            .query_one(
                "SELECT EXISTS (
                     SELECT 1 FROM public.memory_assertions a
                     WHERE a.tenant_id = $1
                       AND a.archived = false
                       AND a.updated_at > COALESCE(
                           (SELECT watermark_at FROM public.memory_curator_state
                            WHERE tenant_id = $1 AND stage = $2),
                           to_timestamp(0)
                       )
                 )",
                &[&tenant_id, &stage],
            )
            .await?;
        Ok(row.get(0))
    }

    /// Advance a curator stage watermark to `now()`.
    pub async fn advance_watermark(&self, tenant_id: &str, stage: &str) -> Result<()> {
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        client
            .execute(
                "INSERT INTO public.memory_curator_state (tenant_id, stage, watermark_at, last_run_at)
                 VALUES ($1, $2, now(), now())
                 ON CONFLICT (tenant_id, stage) DO UPDATE
                 SET watermark_at = now(), last_run_at = now()",
                &[&tenant_id, &stage],
            )
            .await?;
        Ok(())
    }

    /// Stage 2: Merge near-duplicate assertions within the **same**
    /// `(readers, untrusted, private, kind)` equivalence class when cosine
    /// similarity >= `DEDUP_COSINE_THRESHOLD`, unioning `memory_evidence` onto
    /// the surviving assertion and deleting the duplicate.
    pub async fn run_semantic_dedup(&self, tenant_id: &str) -> Result<u64> {
        if !self.stage_has_dirty_work(tenant_id, "dedup").await? {
            return Ok(0);
        }
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let tx = client.transaction().await?;

        let max_dist = 1.0 - DEDUP_COSINE_THRESHOLD;
        let pairs = tx
            .query(
                "SELECT a.assertion_id AS keep_id, b.assertion_id AS drop_id
                 FROM public.memory_assertions a
                 JOIN public.memory_assertions b
                   ON a.tenant_id = b.tenant_id
                  AND a.kind = b.kind
                  AND a.readers = b.readers
                  AND a.untrusted = b.untrusted
                  AND a.private = b.private
                  AND a.archived = false AND b.archived = false
                  AND a.embedding IS NOT NULL AND b.embedding IS NOT NULL
                  AND a.model = b.model
                  AND a.created_at > b.created_at
                  AND (a.embedding <=> b.embedding) <= $2
                 WHERE a.tenant_id = $1
                 LIMIT 100",
                &[&tenant_id, &max_dist],
            )
            .await
            .context("find semantic duplicate assertions")?;

        let mut merged = 0u64;
        for r in pairs {
            let keep_id: String = r.get(0);
            let drop_id: String = r.get(1);

            // Union evidence from dropped duplicate onto the surviving assertion.
            tx.execute(
                "INSERT INTO public.memory_evidence (tenant_id, target_kind, target_id, source_id, source_rev)
                 SELECT tenant_id, 'assertion', $2, source_id, source_rev
                 FROM public.memory_evidence
                 WHERE tenant_id = $1 AND target_kind = 'assertion' AND target_id = $3
                 ON CONFLICT DO NOTHING",
                &[&tenant_id, &keep_id, &drop_id],
            )
            .await?;

            // Re-point edges referencing `drop_id` to `keep_id` before removing `drop_id`.
            tx.execute("UPDATE public.memory_edges SET src_id = $2 WHERE tenant_id = $1 AND src_id = $3", &[&tenant_id, &keep_id, &drop_id]).await?;
            tx.execute("UPDATE public.memory_edges SET dst_id = $2 WHERE tenant_id = $1 AND dst_id = $3", &[&tenant_id, &keep_id, &drop_id]).await?;

            let del = tx.execute("DELETE FROM public.memory_assertions WHERE tenant_id = $1 AND assertion_id = $2", &[&tenant_id, &drop_id]).await?;
            if del > 0 {
                tx.execute(
                    "DELETE FROM public.memory_evidence
                     WHERE tenant_id = $1 AND target_kind = 'assertion' AND target_id = $2",
                    &[&tenant_id, &drop_id],
                )
                .await?;
                merged += del;
            }
        }

        tx.execute(
            "INSERT INTO public.memory_curator_state (tenant_id, stage, watermark_at, last_run_at)
             VALUES ($1, 'dedup', now(), now())
             ON CONFLICT (tenant_id, stage) DO UPDATE
             SET watermark_at = now(), last_run_at = now()",
            &[&tenant_id],
        )
        .await?;

        tx.commit().await?;
        Ok(merged)
    }

    /// Stage 3a: Find high-similarity assertion collisions since the `conflicts`
    /// watermark that do not yet have a `supersedes` or `contradicts` edge.
    pub async fn find_collision_candidates(&self, tenant_id: &str) -> Result<Vec<CollisionPair>> {
        if !self.stage_has_dirty_work(tenant_id, "conflicts").await? {
            return Ok(Vec::new());
        }
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let max_dist = 1.0 - CONFLICT_COSINE_THRESHOLD;
        let rows = client
            .query(
                "SELECT n.assertion_id, n.statement, n.untrusted, n.private,
                        o.assertion_id, o.statement, o.untrusted, o.private,
                        (1.0 - (n.embedding <=> o.embedding)) AS sim
                 FROM public.memory_assertions n
                 JOIN public.memory_assertions o
                   ON n.tenant_id = o.tenant_id
                  AND n.assertion_id <> o.assertion_id
                  AND n.archived = false AND o.archived = false
                  AND n.embedding IS NOT NULL AND o.embedding IS NOT NULL
                  AND n.model = o.model
                  AND n.created_at > o.created_at
                  AND (n.embedding <=> o.embedding) <= $2
                 WHERE n.tenant_id = $1
                   AND n.updated_at > COALESCE(
                       (SELECT watermark_at FROM public.memory_curator_state
                        WHERE tenant_id = $1 AND stage = 'conflicts'),
                       to_timestamp(0)
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM public.memory_edges e
                       WHERE e.tenant_id = $1
                         AND e.rel_type IN ('supersedes', 'contradicts')
                         AND ((e.src_id = n.assertion_id AND e.dst_id = o.assertion_id)
                              OR (e.src_id = o.assertion_id AND e.dst_id = n.assertion_id))
                   )
                 ORDER BY sim DESC
                 LIMIT 50",
                &[&tenant_id, &max_dist],
            )
            .await?;

        Ok(rows
            .into_iter()
            .map(|r| CollisionPair {
                newer_id: r.get(0),
                newer_statement: r.get(1),
                newer_label: TrustLabel { untrusted: r.get(2), private: r.get(3) },
                older_id: r.get(4),
                older_statement: r.get(5),
                older_label: TrustLabel { untrusted: r.get(6), private: r.get(7) },
                cosine_similarity: r.get(8),
            })
            .collect())
    }

    /// Stage 3b: Record a visibility-scoped `supersedes` or `contradicts` edge.
    ///
    /// Trust guardrail: even if the caller requests `Supersede`, if
    /// `!newer_label.may_supersede(older_label)` (e.g. untrusted trying to
    /// supersede clean), it is automatically downgraded to `contradicts`.
    /// The edge inherits `newer`'s `readers` so a private override is visible
    /// *only* to readers authorized to see `newer`.
    pub async fn record_collision_edge(&self, tenant_id: &str, newer_id: &str, older_id: &str, resolution: CollisionResolution, reason: &str) -> Result<Option<String>> {
        if resolution == CollisionResolution::Ignore {
            return Ok(None);
        }
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let tx = client.transaction().await?;

        let n_row = tx
            .query_opt(
                "SELECT readers, untrusted, private FROM public.memory_assertions
                 WHERE tenant_id = $1 AND assertion_id = $2",
                &[&tenant_id, &newer_id],
            )
            .await?;
        let o_row = tx
            .query_opt(
                "SELECT untrusted, private FROM public.memory_assertions
                 WHERE tenant_id = $1 AND assertion_id = $2",
                &[&tenant_id, &older_id],
            )
            .await?;
        let (Some(nr), Some(or)) = (n_row, o_row) else {
            tx.commit().await?;
            return Ok(None);
        };

        let newer_readers: serde_json::Value = nr.get(0);
        let newer_label = TrustLabel { untrusted: nr.get(1), private: nr.get(2) };
        let older_label = TrustLabel { untrusted: or.get(0), private: or.get(1) };

        let rel_type = match resolution {
            CollisionResolution::Supersede if newer_label.may_supersede(older_label) => "supersedes",
            _ => "contradicts",
        };
        let edge_id = format!("edge_{rel_type}_{newer_id}_{older_id}");

        tx.execute(
            "INSERT INTO public.memory_edges (
                 tenant_id, edge_id, src_id, dst_id, rel_type, readers, untrusted, properties
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (tenant_id, edge_id) DO NOTHING",
            &[&tenant_id, &edge_id, &newer_id, &older_id, &rel_type, &newer_readers, &newer_label.untrusted, &json!({ "reason": reason })],
        )
        .await?;

        // Link the edge to `newer_id`'s evidence so revoking `newer_id`'s source
        // automatically revokes the supersession/contradiction edge too.
        tx.execute(
            "INSERT INTO public.memory_evidence (tenant_id, target_kind, target_id, source_id, source_rev)
             SELECT tenant_id, 'edge', $2, source_id, source_rev
             FROM public.memory_evidence
             WHERE tenant_id = $1 AND target_kind = 'assertion' AND target_id = $3
             ON CONFLICT DO NOTHING",
            &[&tenant_id, &edge_id, &newer_id],
        )
        .await?;

        tx.commit().await?;
        Ok(Some(edge_id))
    }

    /// Stage 4: Enforce hard per-scope cap (`MAX_LESSONS_PER_SCOPE`) on distilled
    /// `kind = 'lesson'` assertions by archiving the lowest-ranked overflow lessons.
    pub async fn enforce_lesson_caps(&self, tenant_id: &str) -> Result<u64> {
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let cap = MAX_LESSONS_PER_SCOPE as i64;
        let archived = client
            .execute(
                "WITH ranked AS (
                     SELECT assertion_id,
                            ROW_NUMBER() OVER (
                                PARTITION BY owner_id, readers
                                ORDER BY feedback_weight DESC, access_count DESC, created_at DESC
                            ) AS rn
                     FROM public.memory_assertions
                     WHERE tenant_id = $1 AND kind = 'lesson' AND archived = false
                 )
                 UPDATE public.memory_assertions a
                 SET archived = true, updated_at = now()
                 FROM ranked r
                 WHERE a.tenant_id = $1
                   AND a.assertion_id = r.assertion_id
                   AND r.rn > $2",
                &[&tenant_id, &cap],
            )
            .await
            .context("enforce MAX_LESSONS_PER_SCOPE")?;
        Ok(archived)
    }

    /// Stage 5: Archive cold, unused, low-feedback `untrusted` assertions older
    /// than `COLD_UNTRUSTED_DAYS` (0 LLM calls).
    pub async fn archive_cold_untrusted(&self, tenant_id: &str, now: DateTime<Utc>) -> Result<u64> {
        let cutoff = now - chrono::Duration::days(COLD_UNTRUSTED_DAYS);
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let archived = client
            .execute(
                "UPDATE public.memory_assertions
                 SET archived = true, updated_at = now()
                 WHERE tenant_id = $1
                   AND untrusted = true
                   AND archived = false
                   AND created_at < $2
                   AND (last_accessed_at IS NULL OR last_accessed_at < $2)
                   AND feedback_weight <= 1.0",
                &[&tenant_id, &cutoff],
            )
            .await
            .context("archive cold untrusted assertions")?;
        Ok(archived)
    }

    /// Run all zero-LLM curator maintenance stages in one pass.
    pub async fn run_maintenance_cycle(&self, tenant_id: &str) -> Result<CuratorReport> {
        let duplicates_merged = self.run_semantic_dedup(tenant_id).await?;
        let lessons_pruned = self.enforce_lesson_caps(tenant_id).await?;
        let cold_archived = self.archive_cold_untrusted(tenant_id, Utc::now()).await?;
        Ok(CuratorReport { duplicates_merged, conflicts_linked: 0, supersessions_linked: 0, lessons_pruned, cold_archived })
    }
}
