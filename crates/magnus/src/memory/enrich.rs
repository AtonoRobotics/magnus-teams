//! Fenced async enrichment worker primitives: `FOR UPDATE SKIP LOCKED` job
//! claiming with crash-safe lease expiry, strict `readers`/taint merging on
//! conflict, `PROFILE_MAX_BYTES` enforcement, and `FOR SHARE` live-source
//! commit fencing against concurrent hard deletion.

use crate::memory::store::MemoryStore;
use crate::memory::{canonical_entity_id, format_halfvec_literal, intersect_readers, AssertionInput, AssertionKind, EdgeInput, TaintLabel};
use crate::memory_index::PROFILE_MAX_BYTES;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

/// Maximum enrichment retry attempts before marking a job `failed`.
pub const JOB_MAX_ATTEMPTS: i32 = 5;
/// Base retry backoff in seconds (doubled per attempt).
pub const JOB_BACKOFF_BASE_SECS: i64 = 15;
/// Lease duration in seconds for a claimed `running` job before another worker
/// may reclaim it after a crash.
pub const JOB_LEASE_SECS: i64 = 300;

/// Claimed enrichment job ready for embedding + structured extraction.
#[derive(Clone, Debug)]
pub struct ClaimedJob {
    pub tenant_id: String,
    pub job_id: String,
    pub source_id: String,
    pub source_rev: i64,
    pub owner_id: String,
    pub readers: Vec<Vec<String>>,
    pub label: TaintLabel,
    pub text: String,
    pub occurred_at: DateTime<Utc>,
    pub attempts: i32,
}

/// Output produced by an enrichment worker for one claimed source revision.
#[derive(Clone, Debug, Default)]
pub struct EnrichmentOutput {
    pub assertions: Vec<AssertionInput>,
    pub edges: Vec<EdgeInput>,
}

/// Result of attempting a fenced enrichment commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnrichCommit {
    /// Source revision was still live under `FOR SHARE`; derivations committed.
    Committed,
    /// Source revision was hard-deleted mid-flight; transaction rolled back cleanly.
    FencedDeleted,
}

/// Claim the next due `pending` job (or expired `running` lease) via
/// `FOR UPDATE SKIP LOCKED`.
pub async fn claim_next_job(store: &mut MemoryStore) -> Result<Option<ClaimedJob>> {
    let tx = store.client_mut().transaction().await.context("begin claim_next_job tx")?;

    // Retire any expired leases that already exhausted JOB_MAX_ATTEMPTS.
    tx.execute(
        "UPDATE public.memory_enrichment_jobs
         SET status = 'failed', last_error = 'lease expired after max attempts', locked_until = NULL
         WHERE status = 'running' AND locked_until <= now() AND attempts >= $1",
        &[&JOB_MAX_ATTEMPTS],
    )
    .await?;

    let row = tx
        .query_opt(
            "WITH next_job AS (
                 SELECT j.tenant_id, j.job_id
                 FROM public.memory_enrichment_jobs j
                 WHERE (j.status = 'pending' AND j.next_run_at <= now())
                    OR (j.status = 'running' AND j.locked_until <= now() AND j.attempts < $1)
                 ORDER BY j.next_run_at ASC, j.job_id ASC
                 FOR UPDATE SKIP LOCKED
                 LIMIT 1
             )
             UPDATE public.memory_enrichment_jobs j
             SET status = 'running',
                 attempts = j.attempts + 1,
                 locked_until = now() + ($2::bigint * interval '1 second')
             FROM next_job nj
             WHERE j.tenant_id = nj.tenant_id AND j.job_id = nj.job_id
             RETURNING j.tenant_id, j.job_id, j.source_id, j.source_rev, j.attempts",
            &[&JOB_MAX_ATTEMPTS, &JOB_LEASE_SECS],
        )
        .await?;

    let Some(job_row) = row else {
        tx.commit().await?;
        return Ok(None);
    };

    let tenant_id: String = job_row.get(0);
    let job_id: String = job_row.get(1);
    let source_id: String = job_row.get(2);
    let source_rev: i64 = job_row.get(3);
    let attempts: i32 = job_row.get(4);

    let Some(src_row) = tx
        .query_opt(
            "SELECT owner_id, readers, untrusted, private, text, occurred_at
             FROM public.memory_sources
             WHERE tenant_id = $1 AND source_id = $2 AND rev = $3",
            &[&tenant_id, &source_id, &source_rev],
        )
        .await?
    else {
        tx.rollback().await?;
        return Ok(None);
    };

    let readers_val: Value = src_row.get(1);
    let readers: Vec<Vec<String>> = serde_json::from_value(readers_val).context("decode source readers")?;
    let claimed = ClaimedJob {
        tenant_id,
        job_id,
        source_id,
        source_rev,
        owner_id: src_row.get(0),
        readers,
        label: TaintLabel { untrusted: src_row.get(2), private: src_row.get(3) },
        text: src_row.get(4),
        occurred_at: src_row.get(5),
        attempts,
    };
    tx.commit().await?;
    Ok(Some(claimed))
}

/// Commit extracted assertions, entities, edges, and evidence in a single
/// transaction fenced by `SELECT ... FOR SHARE` on the live `memory_sources` row.
/// If an assertion or edge ID already exists, joins `(untrusted, private)` and
/// intersects `readers` before attaching evidence.
pub async fn commit_enrichment(store: &mut MemoryStore, job: &ClaimedJob, output: &EnrichmentOutput) -> Result<EnrichCommit> {
    let embedding_dim = store.embedding_dim();
    let tx = store.client_mut().transaction().await.context("begin commit_enrichment tx")?;

    // Fencing check: lock the live source revision in SHARE mode so a concurrent
    // FOR UPDATE hard DELETE cannot race past this commit.
    let live = tx
        .query_opt(
            "SELECT readers, untrusted, private
             FROM public.memory_sources
             WHERE tenant_id = $1 AND source_id = $2 AND rev = $3
             FOR SHARE",
            &[&job.tenant_id, &job.source_id, &job.source_rev],
        )
        .await?;

    let Some(src_row) = live else {
        tx.rollback().await?;
        return Ok(EnrichCommit::FencedDeleted);
    };

    let src_readers_val: Value = src_row.get(0);
    let src_readers: Vec<Vec<String>> = serde_json::from_value(src_readers_val.clone()).context("decode live readers")?;
    let src_label = TaintLabel { untrusted: src_row.get(1), private: src_row.get(2) };

    for a in &output.assertions {
        if a.kind == AssertionKind::Profile {
            anyhow::ensure!(!src_label.untrusted, "refusing to write untrusted source into kind = 'profile'");
            anyhow::ensure!(a.statement.len() <= PROFILE_MAX_BYTES, "profile entry exceeds PROFILE_MAX_BYTES ({PROFILE_MAX_BYTES} bytes)");
        }

        let existing = tx
            .query_opt(
                "SELECT kind, readers, untrusted, private
                 FROM public.memory_assertions
                 WHERE tenant_id = $1 AND assertion_id = $2
                 FOR UPDATE",
                &[&job.tenant_id, &a.assertion_id],
            )
            .await?;

        if let Some(ex) = existing {
            let ex_kind: String = ex.get(0);
            let ex_readers: Vec<Vec<String>> = serde_json::from_value(ex.get::<_, Value>(1)).context("decode existing assertion readers")?;
            let ex_label = TaintLabel { untrusted: ex.get(2), private: ex.get(3) };
            let joined = ex_label.join(src_label);
            anyhow::ensure!(!(ex_kind == AssertionKind::Profile.as_str() && joined.untrusted), "refusing to taint existing profile assertion with untrusted source");
            let merged_readers = json!(intersect_readers(&[ex_readers, src_readers.clone()]));
            tx.execute(
                "UPDATE public.memory_assertions
                 SET readers = $3, untrusted = $4, private = $5
                 WHERE tenant_id = $1 AND assertion_id = $2",
                &[&job.tenant_id, &a.assertion_id, &merged_readers, &joined.untrusted, &joined.private],
            )
            .await
            .context("merge taint/readers on existing memory_assertions")?;
        } else {
            if a.kind == AssertionKind::Profile {
                let existing_bytes: i64 = tx
                    .query_one(
                        "SELECT COALESCE(SUM(octet_length(statement)), 0)::bigint
                         FROM public.memory_assertions
                         WHERE tenant_id = $1 AND owner_id = $2 AND kind = 'profile' AND NOT archived",
                        &[&job.tenant_id, &a.owner_id],
                    )
                    .await?
                    .get(0);
                let total = (existing_bytes as usize).saturating_add(a.statement.len());
                anyhow::ensure!(total <= PROFILE_MAX_BYTES, "user profile total ({total} bytes) exceeds PROFILE_MAX_BYTES ({PROFILE_MAX_BYTES} bytes)");
            }

            let emb_lit = match &a.embedding {
                Some(vec) => Some(format_halfvec_literal(vec, embedding_dim)?),
                None => None,
            };
            tx.execute(
                "INSERT INTO public.memory_assertions (
                     tenant_id, assertion_id, kind, owner_id, readers, untrusted, private,
                     statement, valid_from, valid_to, confidence, tsv, embedding
                 ) VALUES (
                     $1, $2, $3, $4, $5, $6, $7,
                     $8, $9, $10, $11, to_tsvector('simple', $8), $12::text::halfvec
                 )",
                &[
                    &job.tenant_id,
                    &a.assertion_id,
                    &a.kind.as_str(),
                    &a.owner_id,
                    &src_readers_val,
                    &src_label.untrusted,
                    &src_label.private,
                    &a.statement,
                    &a.valid_from,
                    &a.valid_to,
                    &a.confidence,
                    &emb_lit,
                ],
            )
            .await
            .context("insert memory_assertions")?;
        }

        tx.execute(
            "INSERT INTO public.memory_evidence (tenant_id, target_kind, target_id, source_id, source_rev)
             VALUES ($1, 'assertion', $2, $3, $4)
             ON CONFLICT DO NOTHING",
            &[&job.tenant_id, &a.assertion_id, &job.source_id, &job.source_rev],
        )
        .await
        .context("insert assertion memory_evidence")?;
    }

    for edge in &output.edges {
        let src_id = canonical_entity_id(&job.tenant_id, &edge.src);
        let dst_id = canonical_entity_id(&job.tenant_id, &edge.dst);

        for (eid, ent) in [(&src_id, &edge.src), (&dst_id, &edge.dst)] {
            tx.execute(
                "INSERT INTO public.memory_entities (tenant_id, entity_id, entity_type, external_id)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (tenant_id, entity_id) DO NOTHING",
                &[&job.tenant_id, eid, &ent.entity_type, &ent.external_id],
            )
            .await
            .context("upsert thin memory_entities")?;
        }

        let existing_edge = tx
            .query_opt(
                "SELECT readers, untrusted, private
                 FROM public.memory_edges
                 WHERE tenant_id = $1 AND edge_id = $2
                 FOR UPDATE",
                &[&job.tenant_id, &edge.edge_id],
            )
            .await?;

        if let Some(ex) = existing_edge {
            let ex_readers: Vec<Vec<String>> = serde_json::from_value(ex.get::<_, Value>(0)).context("decode existing edge readers")?;
            let ex_label = TaintLabel { untrusted: ex.get(1), private: ex.get(2) };
            let joined = ex_label.join(src_label);
            let merged_readers = json!(intersect_readers(&[ex_readers, src_readers.clone()]));
            tx.execute(
                "UPDATE public.memory_edges
                 SET readers = $3, untrusted = $4, private = $5
                 WHERE tenant_id = $1 AND edge_id = $2",
                &[&job.tenant_id, &edge.edge_id, &merged_readers, &joined.untrusted, &joined.private],
            )
            .await
            .context("merge taint/readers on existing memory_edges")?;
        } else {
            tx.execute(
                "INSERT INTO public.memory_edges (
                     tenant_id, edge_id, src_id, dst_id, src_name, dst_name, rel_type, readers,
                     untrusted, private, valid_from, valid_to, properties
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
                &[
                    &job.tenant_id,
                    &edge.edge_id,
                    &src_id,
                    &dst_id,
                    &edge.src.display_name,
                    &edge.dst.display_name,
                    &edge.rel_type,
                    &src_readers_val,
                    &src_label.untrusted,
                    &src_label.private,
                    &edge.valid_from,
                    &edge.valid_to,
                    &edge.properties,
                ],
            )
            .await
            .context("insert memory_edges")?;
        }

        tx.execute(
            "INSERT INTO public.memory_evidence (tenant_id, target_kind, target_id, source_id, source_rev)
             VALUES ($1, 'edge', $2, $3, $4)
             ON CONFLICT DO NOTHING",
            &[&job.tenant_id, &edge.edge_id, &job.source_id, &job.source_rev],
        )
        .await
        .context("insert edge memory_evidence")?;
    }

    tx.execute(
        "UPDATE public.memory_enrichment_jobs
         SET status = 'done', last_error = NULL, locked_until = NULL
         WHERE tenant_id = $1 AND job_id = $2",
        &[&job.tenant_id, &job.job_id],
    )
    .await
    .context("mark enrichment job done")?;

    tx.commit().await?;
    Ok(EnrichCommit::Committed)
}

/// Record a transient enrichment failure with exponential backoff, or mark
/// `failed` once `JOB_MAX_ATTEMPTS` is reached.
pub async fn fail_job(store: &mut MemoryStore, job: &ClaimedJob, error_msg: &str) -> Result<()> {
    let shift = u32::try_from(job.attempts.saturating_sub(1).max(0)).unwrap_or(0).min(10);
    let backoff_secs = JOB_BACKOFF_BASE_SECS.saturating_mul(1_i64 << shift);
    let next_status = if job.attempts >= JOB_MAX_ATTEMPTS { "failed" } else { "pending" };
    let msg = json!(error_msg).to_string();
    store
        .client()
        .execute(
            "UPDATE public.memory_enrichment_jobs
             SET status = $3,
                 last_error = $4,
                 locked_until = NULL,
                 next_run_at = now() + ($5::bigint * interval '1 second')
             WHERE tenant_id = $1 AND job_id = $2",
            &[&job.tenant_id, &job.job_id, &next_status, &msg, &backoff_secs],
        )
        .await
        .context("update failed enrichment job")?;
    Ok(())
}
