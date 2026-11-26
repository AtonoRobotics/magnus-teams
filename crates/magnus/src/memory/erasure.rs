//! Atomic single-transaction source revocation and GDPR person erasure.
//!
//! Deleting a source cascades through `memory_evidence` and atomically removes
//! every assertion, edge, entity, or profile entry whose surviving evidence
//! count drops to zero, while flagging multi-source distilled lessons as stale
//! for lazy rebuild from surviving evidence.

use crate::memory::MemoryEngine;
use anyhow::{Context, Result};

/// Counts of records removed or invalidated in a single revocation/erasure tx.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RevocationReport {
    pub sources_deleted: u64,
    pub assertions_deleted: u64,
    pub assertions_marked_stale: u64,
    pub edges_deleted: u64,
    pub entities_deleted: u64,
    pub profiles_deleted: u64,
}

impl MemoryEngine {
    /// Atomically revoke all revisions (or a specific revision) of `source_id`.
    ///
    /// Because `memory_evidence` and `memory_enrichment_jobs` have composite
    /// `ON DELETE CASCADE` FKs to `memory_sources`, deleting the source row
    /// simultaneously fences any in-flight enrichment worker and removes its
    /// evidence links. Any derived assertion/edge/entity with zero surviving
    /// evidence is deleted in the same transaction; surviving multi-source
    /// lessons are marked `stale = true`.
    pub async fn revoke_source(&self, tenant_id: &str, source_id: &str, rev: Option<i64>) -> Result<RevocationReport> {
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let tx = client.transaction().await.context("begin revoke_source transaction")?;

        // Snapshot targets that currently depend on the doomed source(s).
        let affected_assertions: Vec<String> = tx
            .query(
                "SELECT DISTINCT target_id FROM public.memory_evidence
                 WHERE tenant_id = $1 AND target_kind = 'assertion' AND source_id = $2
                   AND ($3::bigint IS NULL OR source_rev = $3)",
                &[&tenant_id, &source_id, &rev],
            )
            .await?
            .into_iter()
            .map(|r| r.get(0))
            .collect();

        let affected_edges: Vec<String> = tx
            .query(
                "SELECT DISTINCT target_id FROM public.memory_evidence
                 WHERE tenant_id = $1 AND target_kind = 'edge' AND source_id = $2
                   AND ($3::bigint IS NULL OR source_rev = $3)",
                &[&tenant_id, &source_id, &rev],
            )
            .await?
            .into_iter()
            .map(|r| r.get(0))
            .collect();

        let affected_entities: Vec<String> = tx
            .query(
                "SELECT DISTINCT target_id FROM public.memory_evidence
                 WHERE tenant_id = $1 AND target_kind = 'entity' AND source_id = $2
                   AND ($3::bigint IS NULL OR source_rev = $3)",
                &[&tenant_id, &source_id, &rev],
            )
            .await?
            .into_iter()
            .map(|r| r.get(0))
            .collect();

        // Delete the source row(s) — cascades memory_evidence & memory_enrichment_jobs.
        let sources_deleted = tx
            .execute(
                "DELETE FROM public.memory_sources
                 WHERE tenant_id = $1 AND source_id = $2
                   AND ($3::bigint IS NULL OR rev = $3)",
                &[&tenant_id, &source_id, &rev],
            )
            .await
            .context("delete memory_sources")?;

        // Delete assertions with 0 surviving evidence; mark multi-source survivors stale.
        let assertions_deleted = if affected_assertions.is_empty() {
            0
        } else {
            tx.execute(
                "DELETE FROM public.memory_assertions a
                 WHERE a.tenant_id = $1
                   AND a.assertion_id = ANY($2)
                   AND NOT EXISTS (
                       SELECT 1 FROM public.memory_evidence e
                       WHERE e.tenant_id = a.tenant_id
                         AND e.target_kind = 'assertion'
                         AND e.target_id = a.assertion_id
                   )",
                &[&tenant_id, &affected_assertions],
            )
            .await?
        };

        let assertions_marked_stale = if affected_assertions.is_empty() {
            0
        } else {
            tx.execute(
                "UPDATE public.memory_assertions
                 SET stale = true, updated_at = now()
                 WHERE tenant_id = $1 AND assertion_id = ANY($2)",
                &[&tenant_id, &affected_assertions],
            )
            .await?
        };

        // Delete edges with 0 surviving evidence (including private `supersedes` edges!).
        let edges_deleted = if affected_edges.is_empty() {
            0
        } else {
            tx.execute(
                "DELETE FROM public.memory_edges e
                 WHERE e.tenant_id = $1
                   AND e.edge_id = ANY($2)
                   AND NOT EXISTS (
                       SELECT 1 FROM public.memory_evidence ev
                       WHERE ev.tenant_id = e.tenant_id
                         AND ev.target_kind = 'edge'
                         AND ev.target_id = e.edge_id
                   )",
                &[&tenant_id, &affected_edges],
            )
            .await?
        };

        // Delete entities with 0 surviving evidence.
        let entities_deleted = if affected_entities.is_empty() {
            0
        } else {
            tx.execute(
                "DELETE FROM public.memory_entities ent
                 WHERE ent.tenant_id = $1
                   AND ent.entity_id = ANY($2)
                   AND NOT EXISTS (
                       SELECT 1 FROM public.memory_evidence ev
                       WHERE ev.tenant_id = ent.tenant_id
                         AND ev.target_kind = 'entity'
                         AND ev.target_id = ent.entity_id
                   )",
                &[&tenant_id, &affected_entities],
            )
            .await?
        };

        tx.commit().await.context("commit revoke_source transaction")?;
        Ok(RevocationReport { sources_deleted, assertions_deleted, assertions_marked_stale, edges_deleted, entities_deleted, profiles_deleted: 0 })
    }

    /// Atomically erase all personal sources, derivations, and standing profile
    /// owned by `person` (preserving shared org sources/assertions owned by others).
    pub async fn erase_person(&self, tenant_id: &str, person: &str) -> Result<RevocationReport> {
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let tx = client.transaction().await.context("begin erase_person transaction")?;

        let person_principal = format!("person:{person}");
        let sole_readers = serde_json::json!([[person_principal]]);

        let sources_deleted = tx
            .execute(
                "DELETE FROM public.memory_sources
                 WHERE tenant_id = $1 AND (owner_id = $2 OR readers = $3)",
                &[&tenant_id, &person, &sole_readers],
            )
            .await?;

        let assertions_deleted = tx
            .execute(
                "DELETE FROM public.memory_assertions a
                 WHERE a.tenant_id = $1
                   AND (
                       a.owner_id = $2
                       OR a.readers = $3
                       OR NOT EXISTS (
                           SELECT 1 FROM public.memory_evidence ev
                           WHERE ev.tenant_id = a.tenant_id
                             AND ev.target_kind = 'assertion'
                             AND ev.target_id = a.assertion_id
                       )
                   )",
                &[&tenant_id, &person, &sole_readers],
            )
            .await?;

        let edges_deleted = tx
            .execute(
                "DELETE FROM public.memory_edges e
                 WHERE e.tenant_id = $1
                   AND (
                       e.readers = $2
                       OR NOT EXISTS (
                           SELECT 1 FROM public.memory_evidence ev
                           WHERE ev.tenant_id = e.tenant_id
                             AND ev.target_kind = 'edge'
                             AND ev.target_id = e.edge_id
                       )
                   )",
                &[&tenant_id, &sole_readers],
            )
            .await?;

        let entities_deleted = tx
            .execute(
                "DELETE FROM public.memory_entities ent
                 WHERE ent.tenant_id = $1
                   AND (
                       ent.readers = $2
                       OR NOT EXISTS (
                           SELECT 1 FROM public.memory_evidence ev
                           WHERE ev.tenant_id = ent.tenant_id
                             AND ev.target_kind = 'entity'
                             AND ev.target_id = ent.entity_id
                       )
                   )",
                &[&tenant_id, &sole_readers],
            )
            .await?;

        let profiles_deleted = tx.execute("DELETE FROM public.memory_profile WHERE tenant_id = $1 AND person = $2", &[&tenant_id, &person]).await?;

        tx.commit().await.context("commit erase_person transaction")?;
        Ok(RevocationReport { sources_deleted, assertions_deleted, assertions_marked_stale: 0, edges_deleted, entities_deleted, profiles_deleted })
    }
}

/// Ordered SQL statements for replaying a person erasure on a restored backup
/// (`decisions/2026-11-23-backups-know-every-store-of-a-person.md`).
pub fn backup_erasure_replay_sql() -> &'static [&'static str] {
    &[
        "DELETE FROM public.memory_sources WHERE owner_id = $1 OR readers = jsonb_build_array(jsonb_build_array('person:' || $1))",
        "DELETE FROM public.memory_assertions WHERE owner_id = $1 OR readers = jsonb_build_array(jsonb_build_array('person:' || $1))",
        "DELETE FROM public.memory_edges WHERE readers = jsonb_build_array(jsonb_build_array('person:' || $1))",
        "DELETE FROM public.memory_entities WHERE readers = jsonb_build_array(jsonb_build_array('person:' || $1))",
        "DELETE FROM public.memory_profile WHERE person = $1",
    ]
}
