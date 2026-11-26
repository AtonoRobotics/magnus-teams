//! Synchronous durable write path for the unified memory engine.
//!
//! Commits `memory_sources`, its `tsvector`, and a durable `memory_enrichment_jobs`
//! row in one Postgres transaction so newly written evidence is immediately
//! searchable via FTS while embeddings and graph extraction run asynchronously.

use crate::memory::{MemoryEngine, SourceInput, PROFILE_MAX_BYTES};
use anyhow::{ensure, Context, Result};
use serde_json::json;
use sha2::{Digest, Sha256};

/// Outcome of `ingest_source`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IngestResult {
    pub source_id: String,
    pub rev: i64,
    pub content_hash: String,
    /// True if an identical live revision with the same `content_hash` already existed.
    pub deduplicated: bool,
    pub job_id: Option<i64>,
}

/// Compute a stable SHA-256 content hash for retry-safe ingestion.
pub fn content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}

impl MemoryEngine {
    /// Atomically persist a versioned source record, its FTS vector, and a durable
    /// enrichment job. Exact-content retries for the same `(tenant_id, source_id)`
    /// return `deduplicated = true` without enqueuing a duplicate job.
    pub async fn ingest_source(&self, input: &SourceInput) -> Result<IngestResult> {
        ensure!(!input.source_id.trim().is_empty(), "source_id must not be empty");
        ensure!(input.rev >= 1, "source revision must be >= 1");

        let hash = content_hash(&input.text);
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let tx = client.transaction().await.context("begin ingest transaction")?;

        if let Some(existing) = tx
            .query_opt(
                "SELECT rev FROM public.memory_sources
                 WHERE tenant_id = $1 AND source_id = $2 AND content_hash = $3 AND deleted_at IS NULL
                 ORDER BY rev DESC LIMIT 1",
                &[&input.tenant_id, &input.source_id, &hash],
            )
            .await
            .context("check existing content_hash")?
        {
            let existing_rev: i64 = existing.get(0);
            tx.commit().await?;
            return Ok(IngestResult { source_id: input.source_id.clone(), rev: existing_rev, content_hash: hash, deduplicated: true, job_id: None });
        }

        tx.execute(
            "INSERT INTO public.memory_sources (
                 tenant_id, source_id, rev, owner_id, readers, untrusted, private,
                 kind, content_hash, text, metadata, tsv, occurred_at
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, $7,
                 $8, $9, $10, $11, to_tsvector('simple', $10), $12
             )
             ON CONFLICT (tenant_id, source_id, rev) DO NOTHING",
            &[
                &input.tenant_id,
                &input.source_id,
                &input.rev,
                &input.owner_id,
                &json!(input.readers),
                &input.label.untrusted,
                &input.label.private,
                &input.kind,
                &hash,
                &input.text,
                &input.metadata,
                &input.occurred_at,
            ],
        )
        .await
        .context("insert memory_sources")?;

        let job_row = tx
            .query_one(
                "INSERT INTO public.memory_enrichment_jobs (tenant_id, source_id, source_rev)
                 VALUES ($1, $2, $3)
                 RETURNING job_id",
                &[&input.tenant_id, &input.source_id, &input.rev],
            )
            .await
            .context("enqueue enrichment job")?;
        let job_id: i64 = job_row.get(0);

        tx.commit().await.context("commit ingest transaction")?;
        Ok(IngestResult { source_id: input.source_id.clone(), rev: input.rev, content_hash: hash, deduplicated: false, job_id: Some(job_id) })
    }

    /// Update a person's pinned standing profile (`memory_profile`).
    /// Strictly refuses untrusted callers and enforces `PROFILE_MAX_BYTES`.
    pub async fn upsert_profile(&self, tenant_id: &str, person: &str, body: &str, untrusted_caller: bool, evidence_source: Option<(&str, i64)>) -> Result<()> {
        ensure!(!untrusted_caller, "untrusted runs may not modify memory_profile");
        ensure!(body.len() <= PROFILE_MAX_BYTES, "profile exceeds {PROFILE_MAX_BYTES} byte limit");

        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let tx = client.transaction().await?;
        let readers = json!([[format!("person:{person}")]]);

        if let Some((source_id, rev)) = evidence_source {
            let live = tx
                .query_opt(
                    "SELECT 1 FROM public.memory_sources
                     WHERE tenant_id = $1 AND source_id = $2 AND rev = $3 AND deleted_at IS NULL AND untrusted = false",
                    &[&tenant_id, &source_id, &rev],
                )
                .await?;
            ensure!(live.is_some(), "profile evidence source is missing, deleted, or untrusted");
        }

        tx.execute(
            "INSERT INTO public.memory_profile (tenant_id, person, readers, body, updated_at)
             VALUES ($1, $2, $3, $4, now())
             ON CONFLICT (tenant_id, person) DO UPDATE SET body = EXCLUDED.body, updated_at = now()",
            &[&tenant_id, &person, &readers, &body],
        )
        .await?;

        if let Some((source_id, rev)) = evidence_source {
            tx.execute(
                "INSERT INTO public.memory_evidence (tenant_id, target_kind, target_id, source_id, source_rev)
                 VALUES ($1, 'profile', $2, $3, $4)
                 ON CONFLICT DO NOTHING",
                &[&tenant_id, &person, &source_id, &rev],
            )
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Read a person's pinned standing profile under RLS.
    pub async fn read_profile(&self, ctx: &crate::memory::ReaderContext) -> Result<Option<String>> {
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let tx = client.transaction().await?;
        tx.batch_execute(&format!("SET LOCAL ROLE {}", self.reader_role())).await?;
        tx.execute("SELECT set_config('magnus.tenant', $1, true)", &[&ctx.tenant_id]).await?;
        tx.execute("SELECT set_config('magnus.principals', $1, true)", &[&serde_json::to_string(&ctx.principals)?]).await?;

        let row = tx.query_opt("SELECT body FROM public.memory_profile WHERE tenant_id = $1 AND person = $2", &[&ctx.tenant_id, &ctx.person]).await?;
        tx.commit().await?;
        Ok(row.map(|r| r.get(0)))
    }
}
