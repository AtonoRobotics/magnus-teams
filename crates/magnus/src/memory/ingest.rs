//! Synchronous write path (0 LLM calls): race-free exact-content dedup per
//! `(tenant_id, source_id, content_hash, readers, untrusted, private)` with
//! order-independent canonicalized `readers`, plus atomic job enqueue.

use crate::memory::store::MemoryStore;
use crate::memory::{canonicalize_readers, content_hash, SourceInput};
use anyhow::{Context, Result};
use serde_json::json;
use uuid::Uuid;

/// Outcome of synchronous source ingestion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IngestOutcome {
    /// New source revision committed and enrichment job queued.
    Inserted { job_id: String },
    /// Identical normalized content already exists for the same `source_id` and
    /// `(tenant_id, readers, untrusted, private)` — no duplicate row written.
    Deduplicated { existing_source_id: String, existing_rev: i64 },
}

/// Commit a source document synchronously with FTS vector and enqueue an async
/// enrichment job in a single ACID transaction.
pub async fn ingest_source(store: &mut MemoryStore, input: &SourceInput) -> Result<IngestOutcome> {
    let hash = content_hash(&input.text);
    let readers_json = json!(canonicalize_readers(&input.readers));
    let tx = store.client_mut().transaction().await.context("begin ingest_source tx")?;

    let inserted = tx
        .query_opt(
            "INSERT INTO public.memory_sources (
                 tenant_id, source_id, rev, owner_id, readers, untrusted, private,
                 content_hash, text, tsv, occurred_at
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, $7,
                 $8, $9, to_tsvector('simple', $9), $10
             )
             ON CONFLICT (tenant_id, source_id, content_hash, readers, untrusted, private)
             DO NOTHING
             RETURNING rev",
            &[&input.tenant_id, &input.source_id, &input.rev, &input.owner_id, &readers_json, &input.label.untrusted, &input.label.private, &hash, &input.text, &input.occurred_at],
        )
        .await
        .context("insert memory_sources")?;

    if inserted.is_none() {
        let existing = tx
            .query_one(
                "SELECT source_id, rev FROM public.memory_sources
                 WHERE tenant_id = $1
                   AND source_id = $2
                   AND content_hash = $3
                   AND readers = $4
                   AND untrusted = $5
                   AND private = $6
                 LIMIT 1",
                &[&input.tenant_id, &input.source_id, &hash, &readers_json, &input.label.untrusted, &input.label.private],
            )
            .await
            .context("lookup deduplicated memory_sources row")?;
        let existing_source_id: String = existing.get(0);
        let existing_rev: i64 = existing.get(1);
        tx.commit().await?;
        return Ok(IngestOutcome::Deduplicated { existing_source_id, existing_rev });
    }

    let job_id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO public.memory_enrichment_jobs (tenant_id, job_id, source_id, source_rev)
         VALUES ($1, $2, $3, $4)",
        &[&input.tenant_id, &job_id, &input.source_id, &input.rev],
    )
    .await
    .context("enqueue memory_enrichment_jobs")?;

    tx.commit().await?;
    Ok(IngestOutcome::Inserted { job_id })
}
