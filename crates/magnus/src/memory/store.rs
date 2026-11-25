//! RLS-scoped Postgres connection wrapper and atomic hard-delete revocation.

use crate::memory_index::{HnswIterativeScan, MemoryConfig};
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use tokio_postgres::{Client, Transaction};

/// Postgres store owning a kernel connection and the derived `{role}_reader` role.
pub struct MemoryStore {
    client: Client,
    reader_role: String,
    embedding_dim: usize,
    hnsw_iterative_scan: HnswIterativeScan,
}

impl MemoryStore {
    /// Connect using a kernel database URL and validated `MemoryConfig`.
    pub async fn connect(kernel_url: &str, cfg: &MemoryConfig) -> Result<Self> {
        cfg.check()?;
        let parsed = url::Url::parse(kernel_url).context("parse memory database url")?;
        let role = parsed.username();
        anyhow::ensure!(!role.is_empty() && role.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "unsafe Postgres role name in memory database URL");
        let client = crate::log::connect(kernel_url).await?;
        Ok(Self { client, reader_role: format!("{role}_reader"), embedding_dim: cfg.embedding_dim, hnsw_iterative_scan: cfg.hnsw_iterative_scan })
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn client_mut(&mut self) -> &mut Client {
        &mut self.client
    }

    pub fn reader_role(&self) -> &str {
        &self.reader_role
    }

    pub fn embedding_dim(&self) -> usize {
        self.embedding_dim
    }

    /// Begin a transaction scoped to `{role}_reader` with `magnus.principals`
    /// and `hnsw.iterative_scan` set locally.
    pub async fn begin_reader_tx(&mut self, principals: &[String]) -> Result<Transaction<'_>> {
        let tx = self.client.transaction().await.context("begin reader transaction")?;
        tx.batch_execute(&format!("SET LOCAL ROLE {}", self.reader_role)).await.context("switch to reader role")?;
        let principals_json = serde_json::to_string(principals).context("serialize principals")?;
        tx.execute("SELECT set_config('magnus.principals', $1, true)", &[&principals_json]).await.context("set magnus.principals")?;
        tx.execute("SELECT set_config('hnsw.iterative_scan', $1, true)", &[&self.hnsw_iterative_scan.as_sql_str()]).await.context("set hnsw.iterative_scan")?;
        Ok(tx)
    }

    /// Atomically hard-delete a single `(tenant_id, source_id, rev)` row.
    /// Locks the source `FOR UPDATE` first so no concurrent `FOR SHARE`
    /// enrichment commit can race between reading evidence and deleting the
    /// source; cleans up zero-evidence assertions/edges, multi-source distilled
    /// `lesson`/`profile` entries, their remaining `memory_evidence` rows, and
    /// any newly orphaned `memory_entities`.
    pub async fn hard_delete_source_rev(&mut self, tenant_id: &str, source_id: &str, rev: i64) -> Result<u64> {
        let tx = self.client.transaction().await.context("begin hard_delete_source_rev tx")?;

        // 1. Lock the source exclusively first so any in-flight FOR SHARE
        //    enrichment either finishes before us or sees 0 rows after us.
        let locked = tx
            .query_opt(
                "SELECT 1 FROM public.memory_sources
                 WHERE tenant_id = $1 AND source_id = $2 AND rev = $3
                 FOR UPDATE",
                &[&tenant_id, &source_id, &rev],
            )
            .await?;
        if locked.is_none() {
            tx.commit().await?;
            return Ok(0);
        }

        // 2. Single evidence lookup for all targets referencing this source rev.
        let ev_rows = tx
            .query(
                "SELECT target_kind, target_id FROM public.memory_evidence
                 WHERE tenant_id = $1 AND source_id = $2 AND source_rev = $3",
                &[&tenant_id, &source_id, &rev],
            )
            .await?;
        let mut affected_assertions = Vec::new();
        let mut affected_edges = Vec::new();
        for row in ev_rows {
            let kind: String = row.get(0);
            let target_id: String = row.get(1);
            match kind.as_str() {
                "assertion" => affected_assertions.push(target_id),
                "edge" => affected_edges.push(target_id),
                _ => {}
            }
        }

        // 3. Hard-delete the source (cascading its memory_evidence & jobs).
        let deleted = tx
            .execute(
                "DELETE FROM public.memory_sources
                 WHERE tenant_id = $1 AND source_id = $2 AND rev = $3",
                &[&tenant_id, &source_id, &rev],
            )
            .await?;

        // 4. Hard-delete affected distilled lessons/profile entries unconditionally
        //    and any other assertion with 0 surviving evidence, then purge any
        //    remaining evidence rows pointing to those deleted assertions.
        if !affected_assertions.is_empty() {
            let deleted_assertions: Vec<String> = tx
                .query(
                    "DELETE FROM public.memory_assertions a
                     WHERE a.tenant_id = $1
                       AND a.assertion_id = ANY($2)
                       AND (
                           a.kind IN ('lesson', 'profile')
                           OR NOT EXISTS (
                               SELECT 1 FROM public.memory_evidence e
                               WHERE e.tenant_id = a.tenant_id
                                 AND e.target_kind = 'assertion'
                                 AND e.target_id = a.assertion_id
                           )
                       )
                     RETURNING a.assertion_id",
                    &[&tenant_id, &affected_assertions],
                )
                .await?
                .into_iter()
                .map(|r| r.get::<_, String>(0))
                .collect();

            if !deleted_assertions.is_empty() {
                tx.execute(
                    "DELETE FROM public.memory_evidence
                     WHERE tenant_id = $1 AND target_kind = 'assertion' AND target_id = ANY($2)",
                    &[&tenant_id, &deleted_assertions],
                )
                .await?;
            }
        }

        // 5. Hard-delete zero-evidence edges and sweep any newly orphaned entities.
        if !affected_edges.is_empty() {
            let edge_rows = tx
                .query(
                    "DELETE FROM public.memory_edges ed
                     WHERE ed.tenant_id = $1
                       AND ed.edge_id = ANY($2)
                       AND NOT EXISTS (
                           SELECT 1 FROM public.memory_evidence ev
                           WHERE ev.tenant_id = ed.tenant_id
                             AND ev.target_kind = 'edge'
                             AND ev.target_id = ed.edge_id
                       )
                     RETURNING ed.edge_id, ed.src_id, ed.dst_id",
                    &[&tenant_id, &affected_edges],
                )
                .await?;

            let mut deleted_edge_ids = Vec::with_capacity(edge_rows.len());
            let mut candidate_entities = BTreeSet::new();
            for row in edge_rows {
                deleted_edge_ids.push(row.get::<_, String>(0));
                candidate_entities.insert(row.get::<_, String>(1));
                candidate_entities.insert(row.get::<_, String>(2));
            }

            if !deleted_edge_ids.is_empty() {
                tx.execute(
                    "DELETE FROM public.memory_evidence
                     WHERE tenant_id = $1 AND target_kind = 'edge' AND target_id = ANY($2)",
                    &[&tenant_id, &deleted_edge_ids],
                )
                .await?;
            }

            if !candidate_entities.is_empty() {
                let entity_ids: Vec<String> = candidate_entities.into_iter().collect();
                tx.execute(
                    "DELETE FROM public.memory_entities ent
                     WHERE ent.tenant_id = $1
                       AND ent.entity_id = ANY($2)
                       AND NOT EXISTS (
                           SELECT 1 FROM public.memory_edges e
                           WHERE e.tenant_id = ent.tenant_id
                             AND (e.src_id = ent.entity_id OR e.dst_id = ent.entity_id)
                       )",
                    &[&tenant_id, &entity_ids],
                )
                .await?;
            }
        }

        tx.commit().await?;
        Ok(deleted)
    }
}
