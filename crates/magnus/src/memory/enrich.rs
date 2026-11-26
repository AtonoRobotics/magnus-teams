//! Fenced asynchronous enrichment worker for the unified memory engine.
//!
//! Claims durable jobs from `memory_enrichment_jobs`, writes source embeddings
//! and derived assertions/entities/edges/evidence, and fences every commit
//! against `memory_sources.deleted_at IS NULL` so an in-flight worker can never
//! resurrect revoked or erased content.

use crate::memory::{format_vector, intersect_readers, AssertionInput, EdgeInput, EntityInput, MemoryEngine, TrustLabel, ENRICHMENT_RETRY_BASE, MAX_ENRICHMENT_ATTEMPTS};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// A claimed enrichment job pointing to a specific immutable source revision.
#[derive(Clone, Debug)]
pub struct ClaimedJob {
    pub job_id: i64,
    pub tenant_id: String,
    pub source_id: String,
    pub source_rev: i64,
    pub text: String,
    pub owner_id: Option<String>,
    pub readers: Vec<Vec<String>>,
    pub label: TrustLabel,
}

/// Output produced by an embedder/extractor for a source revision.
#[derive(Clone, Debug, Default)]
pub struct EnrichmentPayload {
    pub source_embedding: Option<Vec<f32>>,
    pub model: Option<String>,
    pub assertions: Vec<AssertionInput>,
    pub entities: Vec<EntityInput>,
    pub edges: Vec<EdgeInput>,
}

/// Outcome of attempting to commit an enrichment payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitOutcome {
    Committed,
    /// Aborted cleanly because the source revision was deleted/revoked while in flight.
    FencedSourceDeleted,
}

/// Normalize an entity display name for candidate lookup.
pub fn normalize_entity_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Deterministic fallback entity ID when no external ID is present.
pub fn deterministic_entity_id(tenant_id: &str, entity_type: &str, key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(tenant_id.as_bytes());
    hasher.update(b"\x1f");
    hasher.update(entity_type.to_lowercase().as_bytes());
    hasher.update(b"\x1f");
    hasher.update(normalize_entity_name(key).as_bytes());
    let digest = hex::encode(hasher.finalize());
    format!("ent_{}", &digest[..24])
}

impl MemoryEngine {
    /// Claim the next pending enrichment job using `FOR UPDATE SKIP LOCKED`.
    pub async fn claim_next_job(&self) -> Result<Option<ClaimedJob>> {
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let tx = client.transaction().await?;

        let row = tx
            .query_opt(
                "SELECT j.job_id, j.tenant_id, j.source_id, j.source_rev,
                        s.text, s.owner_id, s.readers, s.untrusted, s.private
                 FROM public.memory_enrichment_jobs j
                 JOIN public.memory_sources s
                   ON s.tenant_id = j.tenant_id
                  AND s.source_id = j.source_id
                  AND s.rev = j.source_rev
                 WHERE j.status = 'pending'
                   AND j.next_run_at <= now()
                   AND s.deleted_at IS NULL
                 ORDER BY j.next_run_at ASC, j.job_id ASC
                 LIMIT 1
                 FOR UPDATE OF j SKIP LOCKED",
                &[],
            )
            .await
            .context("claim enrichment job")?;

        let Some(r) = row else {
            tx.commit().await?;
            return Ok(None);
        };

        let job_id: i64 = r.get(0);
        tx.execute(
            "UPDATE public.memory_enrichment_jobs
             SET status = 'running', attempts = attempts + 1, updated_at = now()
             WHERE job_id = $1",
            &[&job_id],
        )
        .await?;
        tx.commit().await?;

        let readers_val: Value = r.get(6);
        let readers: Vec<Vec<String>> = serde_json::from_value(readers_val).unwrap_or_default();
        Ok(Some(ClaimedJob {
            job_id,
            tenant_id: r.get(1),
            source_id: r.get(2),
            source_rev: r.get(3),
            text: r.get(4),
            owner_id: r.get(5),
            readers,
            label: TrustLabel { untrusted: r.get(7), private: r.get(8) },
        }))
    }

    /// Commit extracted embeddings, assertions, entities, edges, and evidence
    /// atomically — fenced against `memory_sources.deleted_at IS NULL`.
    ///
    /// Derived rows inherit the strictest `readers` intersection and logical OR
    /// of `untrusted`/`private` from the live source revision.
    pub async fn commit_enrichment(&self, tenant_id: &str, source_id: &str, source_rev: i64, job_id: Option<i64>, payload: &EnrichmentPayload) -> Result<CommitOutcome> {
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let tx = client.transaction().await?;

        // Fence: lock the live source revision for the duration of this commit.
        // If a concurrent revoke/erase deleted or soft-deleted it, abort cleanly.
        let live = tx
            .query_opt(
                "SELECT owner_id, readers, untrusted, private
                 FROM public.memory_sources
                 WHERE tenant_id = $1 AND source_id = $2 AND rev = $3 AND deleted_at IS NULL
                 FOR SHARE",
                &[&tenant_id, &source_id, &source_rev],
            )
            .await
            .context("fence enrichment commit against live source")?;

        let Some(src_row) = live else {
            if let Some(jid) = job_id {
                let _ = tx
                    .execute(
                        "UPDATE public.memory_enrichment_jobs
                         SET status = 'aborted_deleted', updated_at = now()
                         WHERE job_id = $1",
                        &[&jid],
                    )
                    .await;
            }
            tx.commit().await?;
            return Ok(CommitOutcome::FencedSourceDeleted);
        };

        let owner_id: Option<String> = src_row.get(0);
        let src_readers_val: Value = src_row.get(1);
        let src_readers: Vec<Vec<String>> = serde_json::from_value(src_readers_val).unwrap_or_default();
        let src_label = TrustLabel { untrusted: src_row.get(2), private: src_row.get(3) };

        // 1. Update source embedding if provided.
        if let Some(vec) = &payload.source_embedding {
            let vec_lit = format_vector(vec);
            tx.execute(
                "UPDATE public.memory_sources
                 SET embedding = $4::text::vector, model = $5
                 WHERE tenant_id = $1 AND source_id = $2 AND rev = $3 AND deleted_at IS NULL",
                &[&tenant_id, &source_id, &source_rev, &vec_lit, &payload.model],
            )
            .await
            .context("update memory_sources embedding")?;
        }

        // 2. Upsert canonical entities (authoritative external_id first, then norm_name).
        for ent in &payload.entities {
            let norm = normalize_entity_name(&ent.canonical_name);
            let existing_id: Option<String> = if let Some(ext) = &ent.external_id {
                tx.query_opt(
                    "SELECT entity_id FROM public.memory_entities
                     WHERE tenant_id = $1 AND entity_type = $2 AND external_id = $3",
                    &[&tenant_id, &ent.entity_type, ext],
                )
                .await?
                .map(|r| r.get(0))
            } else {
                tx.query_opt(
                    "SELECT entity_id FROM public.memory_entities
                     WHERE tenant_id = $1 AND entity_type = $2 AND norm_name = $3
                     LIMIT 1",
                    &[&tenant_id, &ent.entity_type, &norm],
                )
                .await?
                .map(|r| r.get(0))
            };

            let entity_id = existing_id.unwrap_or_else(|| {
                let key = ent.external_id.as_deref().unwrap_or(&ent.canonical_name);
                deterministic_entity_id(tenant_id, &ent.entity_type, key)
            });
            let vec_lit = ent.embedding.as_deref().map(format_vector);
            let ent_readers = json!(src_readers);

            tx.execute(
                "INSERT INTO public.memory_entities (
                     tenant_id, entity_id, entity_type, canonical_name, norm_name,
                     external_id, aliases, readers, embedding, model
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::text::vector, $10)
                 ON CONFLICT (tenant_id, entity_id) DO UPDATE SET
                     external_id = COALESCE(memory_entities.external_id, EXCLUDED.external_id),
                     embedding = COALESCE(EXCLUDED.embedding, memory_entities.embedding),
                     model = COALESCE(EXCLUDED.model, memory_entities.model),
                     updated_at = now()",
                &[&tenant_id, &entity_id, &ent.entity_type, &ent.canonical_name, &norm, &ent.external_id, &json!(ent.aliases), &ent_readers, &vec_lit, &ent.model],
            )
            .await
            .context("upsert memory_entities")?;

            tx.execute(
                "INSERT INTO public.memory_evidence (tenant_id, target_kind, target_id, source_id, source_rev)
                 VALUES ($1, 'entity', $2, $3, $4)
                 ON CONFLICT DO NOTHING",
                &[&tenant_id, &entity_id, &source_id, &source_rev],
            )
            .await?;
        }

        // 3. Upsert assertions with inherited readers + taint and evidence link.
        let inherited_readers = intersect_readers(&src_readers, &[]);
        for a in &payload.assertions {
            let vec_lit = a.embedding.as_deref().map(format_vector);
            tx.execute(
                "INSERT INTO public.memory_assertions (
                     tenant_id, assertion_id, kind, owner_id, readers, untrusted, private,
                     statement, valid_from, valid_to, confidence, tsv, embedding, model
                 ) VALUES (
                     $1, $2, $3, $4, $5, $6, $7,
                     $8, $9, $10, $11, to_tsvector('simple', $8), $12::text::vector, $13
                 )
                 ON CONFLICT (tenant_id, assertion_id) DO UPDATE SET
                     statement = EXCLUDED.statement,
                     valid_from = COALESCE(EXCLUDED.valid_from, memory_assertions.valid_from),
                     valid_to = COALESCE(EXCLUDED.valid_to, memory_assertions.valid_to),
                     tsv = EXCLUDED.tsv,
                     embedding = COALESCE(EXCLUDED.embedding, memory_assertions.embedding),
                     model = COALESCE(EXCLUDED.model, memory_assertions.model),
                     stale = false,
                     updated_at = now()",
                &[
                    &tenant_id,
                    &a.assertion_id,
                    &a.kind,
                    &owner_id,
                    &json!(inherited_readers),
                    &src_label.untrusted,
                    &src_label.private,
                    &a.statement,
                    &a.valid_from,
                    &a.valid_to,
                    &a.confidence,
                    &vec_lit,
                    &a.model,
                ],
            )
            .await
            .context("upsert memory_assertions")?;

            tx.execute(
                "INSERT INTO public.memory_evidence (tenant_id, target_kind, target_id, source_id, source_rev)
                 VALUES ($1, 'assertion', $2, $3, $4)
                 ON CONFLICT DO NOTHING",
                &[&tenant_id, &a.assertion_id, &source_id, &source_rev],
            )
            .await?;
        }

        // 4. Upsert edges with inherited readers + taint and evidence link.
        for e in &payload.edges {
            tx.execute(
                "INSERT INTO public.memory_edges (
                     tenant_id, edge_id, src_id, dst_id, rel_type, readers, untrusted,
                     valid_from, valid_to, properties
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                 ON CONFLICT (tenant_id, edge_id) DO UPDATE SET
                     valid_from = COALESCE(EXCLUDED.valid_from, memory_edges.valid_from),
                     valid_to = COALESCE(EXCLUDED.valid_to, memory_edges.valid_to),
                     properties = EXCLUDED.properties",
                &[&tenant_id, &e.edge_id, &e.src_id, &e.dst_id, &e.rel_type, &json!(inherited_readers), &src_label.untrusted, &e.valid_from, &e.valid_to, &e.properties],
            )
            .await
            .context("upsert memory_edges")?;

            tx.execute(
                "INSERT INTO public.memory_evidence (tenant_id, target_kind, target_id, source_id, source_rev)
                 VALUES ($1, 'edge', $2, $3, $4)
                 ON CONFLICT DO NOTHING",
                &[&tenant_id, &e.edge_id, &source_id, &source_rev],
            )
            .await?;
        }

        if let Some(jid) = job_id {
            tx.execute(
                "UPDATE public.memory_enrichment_jobs
                 SET status = 'done', updated_at = now(), last_error = NULL
                 WHERE job_id = $1",
                &[&jid],
            )
            .await?;
        }

        tx.commit().await.context("commit enrichment transaction")?;
        Ok(CommitOutcome::Committed)
    }

    /// Record a transient or permanent failure on an enrichment job with backoff.
    pub async fn fail_job(&self, job_id: i64, error: &str) -> Result<()> {
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let base_secs = ENRICHMENT_RETRY_BASE.as_secs() as f64;
        client
            .execute(
                "UPDATE public.memory_enrichment_jobs
                 SET status = CASE WHEN attempts >= $2 THEN 'failed' ELSE 'pending' END,
                     next_run_at = now() + make_interval(secs => $3 * power(2, GREATEST(attempts - 1, 0))),
                     last_error = $4,
                     updated_at = now()
                 WHERE job_id = $1",
                &[&job_id, &MAX_ENRICHMENT_ATTEMPTS, &base_secs, &error],
            )
            .await
            .context("record enrichment job failure")?;
        Ok(())
    }
}
