//! RLS-scoped Postgres connection wrapper, guarded supersession, and atomic
//! hard-delete revocation.

use crate::memory::AssertionKind;
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

    /// Begin a transaction scoped to `{role}_reader` with `magnus.tenant`,
    /// `magnus.principals`, and `hnsw.iterative_scan` set locally.
    pub async fn begin_reader_tx(&mut self, tenant_id: &str, principals: &[String]) -> Result<Transaction<'_>> {
        let tx = self.client.transaction().await.context("begin reader transaction")?;
        tx.batch_execute(&format!("SET LOCAL ROLE {}", self.reader_role)).await.context("switch to reader role")?;
        tx.execute("SELECT set_config('magnus.tenant', $1, true)", &[&tenant_id]).await.context("set magnus.tenant")?;
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

        let deleted = tx
            .execute(
                "DELETE FROM public.memory_sources
                 WHERE tenant_id = $1 AND source_id = $2 AND rev = $3",
                &[&tenant_id, &source_id, &rev],
            )
            .await?;

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

    /// Record that `new_assertion_id` supersedes `old_assertion_id` within `tenant_id`.
    /// Enforces:
    /// - No self-supersession or transitive cycles.
    /// - Untrusted assertions may never supersede trusted assertions (Invariant 4).
    /// - Non-profile assertions may never supersede `kind = 'profile'`.
    pub async fn record_supersession(&mut self, tenant_id: &str, new_assertion_id: &str, old_assertion_id: &str) -> Result<()> {
        anyhow::ensure!(new_assertion_id != old_assertion_id, "an assertion cannot supersede itself");
        let tx = self.client.transaction().await.context("begin record_supersession tx")?;

        let new_row = tx
            .query_opt(
                "SELECT kind, untrusted FROM public.memory_assertions
                 WHERE tenant_id = $1 AND assertion_id = $2
                 FOR SHARE",
                &[&tenant_id, &new_assertion_id],
            )
            .await?
            .context("superseding assertion not found")?;
        let old_row = tx
            .query_opt(
                "SELECT kind, untrusted FROM public.memory_assertions
                 WHERE tenant_id = $1 AND assertion_id = $2
                 FOR SHARE",
                &[&tenant_id, &old_assertion_id],
            )
            .await?
            .context("superseded assertion not found")?;

        let new_kind: String = new_row.get(0);
        let new_untrusted: bool = new_row.get(1);
        let old_kind: String = old_row.get(0);
        let old_untrusted: bool = old_row.get(1);

        anyhow::ensure!(!(new_untrusted && !old_untrusted), "untrusted assertion cannot supersede a trusted assertion (Invariant 4)");
        anyhow::ensure!(!(old_kind == AssertionKind::Profile.as_str() && new_kind != AssertionKind::Profile.as_str()), "non-profile assertion cannot supersede a profile entry");

        let creates_cycle = tx
            .query_opt(
                "WITH RECURSIVE chain AS (
                     SELECT old_assertion_id AS cur
                     FROM public.memory_supersessions
                     WHERE tenant_id = $1 AND new_assertion_id = $2
                     UNION
                     SELECT s.old_assertion_id
                     FROM public.memory_supersessions s
                     JOIN chain c ON s.new_assertion_id = c.cur
                     WHERE s.tenant_id = $1
                 )
                 SELECT 1 FROM chain WHERE cur = $3 LIMIT 1",
                &[&tenant_id, &old_assertion_id, &new_assertion_id],
            )
            .await?
            .is_some();
        anyhow::ensure!(!creates_cycle, "supersession would create a cycle");

        tx.execute(
            "INSERT INTO public.memory_supersessions (tenant_id, new_assertion_id, old_assertion_id)
             VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
            &[&tenant_id, &new_assertion_id, &old_assertion_id],
        )
        .await
        .context("insert memory_supersessions")?;

        tx.commit().await?;
        Ok(())
    }
}
