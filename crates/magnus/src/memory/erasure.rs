//! Atomic hard-delete person erasure for the unified Postgres memory engine.

use crate::memory::store::MemoryStore;
use crate::memory_index::MemoryConfig;
use anyhow::{Context, Result};
use std::collections::BTreeSet;

/// Row counts removed by an atomic person erasure transaction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ErasureStats {
    pub sources_deleted: u64,
    pub assertions_deleted: u64,
    pub edges_deleted: u64,
    pub entities_deleted: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EvidenceTargetKind {
    Assertion,
    Edge,
}

impl EvidenceTargetKind {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "assertion" => Some(Self::Assertion),
            "edge" => Some(Self::Edge),
            _ => None,
        }
    }
}

/// Atomically erase all memory rows belonging to or derived from `owner_id`
/// within `tenant_id`. Locks owned `memory_sources` `FOR UPDATE` first so
/// concurrent `FOR SHARE` enrichment or distillation commits cannot race.
pub async fn erase_owner(store: &mut MemoryStore, tenant_id: &str, owner_id: &str) -> Result<ErasureStats> {
    erase_owner_scoped(store, Some(tenant_id), owner_id).await
}

/// Atomically erase all memory rows belonging to or derived from `owner_id`
/// across all tenants in the database (called by `person_admin::erase_person`).
pub async fn erase_owner_across_tenants(store: &mut MemoryStore, owner_id: &str) -> Result<ErasureStats> {
    erase_owner_scoped(store, None, owner_id).await
}

/// Mode-gated entry point for `person_admin::erase_person`: no-op when
/// `MemoryMode::Legacy` everywhere.
pub async fn erase_person_if_enabled(kernel_url: &str, cfg: &MemoryConfig, person: &str) -> Result<ErasureStats> {
    if !cfg.is_unified_enabled() {
        return Ok(ErasureStats::default());
    }
    let mut store = MemoryStore::connect(kernel_url, cfg).await?;
    erase_owner_across_tenants(&mut store, person).await
}

async fn erase_owner_scoped(store: &mut MemoryStore, tenant_filter: Option<&str>, owner_id: &str) -> Result<ErasureStats> {
    let tx = store.client_mut().transaction().await.context("begin erase_owner tx")?;

    // 1. Lock all owned sources FOR UPDATE first to serialize against concurrent
    //    FOR SHARE enrichment and distillation commits.
    let src_rows = tx
        .query(
            "SELECT tenant_id, source_id, rev
             FROM public.memory_sources
             WHERE owner_id = $1
               AND ($2::text IS NULL OR tenant_id = $2)
             FOR UPDATE",
            &[&owner_id, &tenant_filter],
        )
        .await
        .context("lock owned memory_sources FOR UPDATE")?;

    let mut ev_assertion_tenants = Vec::new();
    let mut ev_assertion_ids = Vec::new();
    let mut ev_edge_tenants = Vec::new();
    let mut ev_edge_ids = Vec::new();

    if !src_rows.is_empty() {
        let src_tenants: Vec<String> = src_rows.iter().map(|r| r.get(0)).collect();
        let src_ids: Vec<String> = src_rows.iter().map(|r| r.get(1)).collect();
        let src_revs: Vec<i64> = src_rows.iter().map(|r| r.get(2)).collect();

        // 2. Collect all evidence targets linked to the locked owned sources.
        let ev_rows = tx
            .query(
                "SELECT DISTINCT ev.tenant_id, ev.target_kind, ev.target_id
                 FROM unnest($1::text[], $2::text[], $3::bigint[]) AS s(tenant_id, source_id, rev)
                 JOIN public.memory_evidence ev
                   ON ev.tenant_id = s.tenant_id
                  AND ev.source_id = s.source_id
                  AND ev.source_rev = s.rev",
                &[&src_tenants, &src_ids, &src_revs],
            )
            .await
            .context("collect evidence for owned sources")?;

        for row in ev_rows {
            let tid: String = row.get(0);
            let kind: String = row.get(1);
            let target_id: String = row.get(2);
            match EvidenceTargetKind::parse(&kind) {
                Some(EvidenceTargetKind::Assertion) => {
                    ev_assertion_tenants.push(tid);
                    ev_assertion_ids.push(target_id);
                }
                Some(EvidenceTargetKind::Edge) => {
                    ev_edge_tenants.push(tid);
                    ev_edge_ids.push(target_id);
                }
                None => {}
            }
        }
    }

    // 3. Hard-delete all owned sources (cascading their memory_evidence & jobs).
    let sources_deleted = tx
        .execute(
            "DELETE FROM public.memory_sources
             WHERE owner_id = $1
               AND ($2::text IS NULL OR tenant_id = $2)",
            &[&owner_id, &tenant_filter],
        )
        .await
        .context("delete owned memory_sources")?;

    // 4. Hard-delete all directly owned assertions plus any affected distilled
    //    lesson/profile or zero-evidence assertion, then purge remaining evidence.
    let del_assertion_rows = tx
        .query(
            "WITH affected(tenant_id, assertion_id) AS (
                 SELECT * FROM unnest($3::text[], $4::text[])
             )
             DELETE FROM public.memory_assertions a
             WHERE ($2::text IS NULL OR a.tenant_id = $2)
               AND (
                   a.owner_id = $1
                   OR (
                       EXISTS (
                           SELECT 1 FROM affected af
                           WHERE af.tenant_id = a.tenant_id AND af.assertion_id = a.assertion_id
                       )
                       AND (
                           a.kind IN ('lesson', 'profile')
                           OR NOT EXISTS (
                               SELECT 1 FROM public.memory_evidence e
                               WHERE e.tenant_id = a.tenant_id
                                 AND e.target_kind = 'assertion'
                                 AND e.target_id = a.assertion_id
                           )
                       )
                   )
               )
             RETURNING a.tenant_id, a.assertion_id",
            &[&owner_id, &tenant_filter, &ev_assertion_tenants, &ev_assertion_ids],
        )
        .await
        .context("delete owned and zero-evidence memory_assertions")?;
    let assertions_deleted = del_assertion_rows.len() as u64;

    if !del_assertion_rows.is_empty() {
        let del_t: Vec<String> = del_assertion_rows.iter().map(|r| r.get(0)).collect();
        let del_a: Vec<String> = del_assertion_rows.iter().map(|r| r.get(1)).collect();
        tx.execute(
            "DELETE FROM public.memory_evidence ev
             USING unnest($1::text[], $2::text[]) AS d(tenant_id, assertion_id)
             WHERE ev.tenant_id = d.tenant_id
               AND ev.target_kind = 'assertion'
               AND ev.target_id = d.assertion_id",
            &[&del_t, &del_a],
        )
        .await
        .context("purge remaining evidence for deleted assertions")?;
    }

    // 5. Hard-delete zero-evidence affected edges and sweep newly orphaned entities.
    let mut edges_deleted = 0u64;
    let mut entities_deleted = 0u64;
    if !ev_edge_ids.is_empty() {
        let del_edge_rows = tx
            .query(
                "WITH affected(tenant_id, edge_id) AS (
                     SELECT * FROM unnest($1::text[], $2::text[])
                 )
                 DELETE FROM public.memory_edges ed
                 WHERE EXISTS (
                     SELECT 1 FROM affected af
                     WHERE af.tenant_id = ed.tenant_id AND af.edge_id = ed.edge_id
                 )
                   AND NOT EXISTS (
                       SELECT 1 FROM public.memory_evidence ev
                       WHERE ev.tenant_id = ed.tenant_id
                         AND ev.target_kind = 'edge'
                         AND ev.target_id = ed.edge_id
                   )
                 RETURNING ed.tenant_id, ed.edge_id, ed.src_id, ed.dst_id",
                &[&ev_edge_tenants, &ev_edge_ids],
            )
            .await
            .context("delete zero-evidence memory_edges")?;
        edges_deleted = del_edge_rows.len() as u64;

        if !del_edge_rows.is_empty() {
            let del_et: Vec<String> = del_edge_rows.iter().map(|r| r.get(0)).collect();
            let del_eid: Vec<String> = del_edge_rows.iter().map(|r| r.get(1)).collect();
            tx.execute(
                "DELETE FROM public.memory_evidence ev
                 USING unnest($1::text[], $2::text[]) AS d(tenant_id, edge_id)
                 WHERE ev.tenant_id = d.tenant_id
                   AND ev.target_kind = 'edge'
                   AND ev.target_id = d.edge_id",
                &[&del_et, &del_eid],
            )
            .await
            .context("purge remaining evidence for deleted edges")?;

            let mut candidate_entities = BTreeSet::new();
            for row in del_edge_rows {
                let tid: String = row.get(0);
                candidate_entities.insert((tid.clone(), row.get::<_, String>(2)));
                candidate_entities.insert((tid, row.get::<_, String>(3)));
            }
            let cand_t: Vec<String> = candidate_entities.iter().map(|(t, _)| t.clone()).collect();
            let cand_e: Vec<String> = candidate_entities.iter().map(|(_, e)| e.clone()).collect();
            entities_deleted = tx
                .execute(
                    "DELETE FROM public.memory_entities ent
                     USING unnest($1::text[], $2::text[]) AS c(tenant_id, entity_id)
                     WHERE ent.tenant_id = c.tenant_id
                       AND ent.entity_id = c.entity_id
                       AND NOT EXISTS (
                           SELECT 1 FROM public.memory_edges e
                           WHERE e.tenant_id = ent.tenant_id
                             AND (e.src_id = ent.entity_id OR e.dst_id = ent.entity_id)
                       )",
                    &[&cand_t, &cand_e],
                )
                .await
                .context("sweep orphaned memory_entities after person erasure")?;
        }
    }

    tx.commit().await.context("commit erase_owner tx")?;
    Ok(ErasureStats { sources_deleted, assertions_deleted, edges_deleted, entities_deleted })
}
