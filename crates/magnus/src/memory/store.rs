//! RLS-scoped Postgres connection wrapper, guarded supersession, and atomic
//! hard-delete revocation.

use crate::memory::AssertionKind;
use crate::memory_index::{HnswIterativeScan, MemoryConfig};
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use tokio_postgres::{Client, Transaction};

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
        let role = crate::log::ident(parsed.username())?;
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
            match EvidenceTargetKind::parse(&kind) {
                Some(EvidenceTargetKind::Assertion) => affected_assertions.push(target_id),
                Some(EvidenceTargetKind::Edge) => affected_edges.push(target_id),
                None => {}
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

    /// Hard-delete every revision of `(tenant_id, source_id)` via `hard_delete_source_rev`.
    pub async fn hard_delete_source_all_revs(&mut self, tenant_id: &str, source_id: &str) -> Result<u64> {
        let revs: Vec<i64> = self
            .client
            .query("SELECT rev FROM public.memory_sources WHERE tenant_id = $1 AND source_id = $2", &[&tenant_id, &source_id])
            .await
            .context("list revisions for source deletion")?
            .into_iter()
            .map(|r| r.get(0))
            .collect();
        let mut total = 0u64;
        for rev in revs {
            total += self.hard_delete_source_rev(tenant_id, source_id, rev).await?;
        }
        Ok(total)
    }

    /// True once the unified memory tables have been migrated.
    pub async fn tables_present(&self) -> Result<bool> {
        let row = self.client.query_one("SELECT to_regclass('public.memory_sources') IS NOT NULL", &[]).await.context("check whether the unified memory tables exist")?;
        Ok(row.get(0))
    }

    /// Every owner with at least one source in `tenant_id`.
    pub async fn owners(&self, tenant_id: &str) -> Result<Vec<String>> {
        let rows = self.client.query("SELECT DISTINCT owner_id FROM public.memory_sources WHERE tenant_id = $1", &[&tenant_id]).await.context("list unified memory owners")?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    /// The source ids `owner_id` has in `tenant_id`, whatever their revision.
    pub async fn source_ids_for_owner(&self, tenant_id: &str, owner_id: &str) -> Result<std::collections::HashSet<String>> {
        let rows = self
            .client
            .query("SELECT DISTINCT source_id FROM public.memory_sources WHERE tenant_id = $1 AND owner_id = $2", &[&tenant_id, &owner_id])
            .await
            .context("list the owner's unified sources")?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    /// Explicitly increment `access_count` and `last_accessed_at` for assertions
    /// returned to a caller (used by cold-archival and lesson-cap ranking).
    pub async fn record_access(&mut self, tenant_id: &str, assertion_ids: &[String]) -> Result<u64> {
        if assertion_ids.is_empty() {
            return Ok(0);
        }
        let updated = self
            .client
            .execute(
                "UPDATE public.memory_assertions
                 SET access_count = access_count + 1, last_accessed_at = now()
                 WHERE tenant_id = $1 AND assertion_id = ANY($2)",
                &[&tenant_id, &assertion_ids],
            )
            .await
            .context("record assertion access counts")?;
        Ok(updated)
    }

    /// Record that `new_assertion_id` supersedes `old_assertion_id` within `tenant_id`.
    /// A guard refusal is an `Ok(Refused(..))`, never an error, so callers can
    /// tell a policy decision from a database failure. Refuses:
    /// - self-supersession and transitive cycles;
    /// - an untrusted assertion superseding a trusted one (Invariant 4);
    /// - a non-profile assertion superseding `kind = 'profile'`;
    /// - either assertion being gone (e.g. merged away earlier in a curator pass).
    pub async fn record_supersession(&mut self, tenant_id: &str, new_assertion_id: &str, old_assertion_id: &str) -> Result<SupersessionOutcome> {
        if new_assertion_id == old_assertion_id {
            return Ok(SupersessionOutcome::Refused(SupersessionRefusal::SelfLink));
        }
        let tx = self.client.transaction().await.context("begin record_supersession tx")?;

        let lock_sql = "SELECT kind, untrusted FROM public.memory_assertions
                        WHERE tenant_id = $1 AND assertion_id = $2
                        FOR SHARE";
        let new_row = tx.query_opt(lock_sql, &[&tenant_id, &new_assertion_id]).await.context("lock superseding assertion")?;
        let old_row = tx.query_opt(lock_sql, &[&tenant_id, &old_assertion_id]).await.context("lock superseded assertion")?;
        let (Some(new_row), Some(old_row)) = (new_row, old_row) else {
            tx.rollback().await?;
            return Ok(SupersessionOutcome::Refused(SupersessionRefusal::Missing));
        };

        let new_kind: AssertionKind = new_row.get::<_, String>(0).parse()?;
        let new_untrusted: bool = new_row.get(1);
        let old_kind: AssertionKind = old_row.get::<_, String>(0).parse()?;
        let old_untrusted: bool = old_row.get(1);

        let refusal = if new_untrusted && !old_untrusted {
            Some(SupersessionRefusal::UntrustedOverTrusted)
        } else if old_kind == AssertionKind::Profile && new_kind != AssertionKind::Profile {
            Some(SupersessionRefusal::NonProfileOverProfile)
        } else if supersession_reaches(&tx, tenant_id, old_assertion_id, new_assertion_id).await? {
            Some(SupersessionRefusal::Cycle)
        } else {
            None
        };
        if let Some(reason) = refusal {
            tx.rollback().await?;
            return Ok(SupersessionOutcome::Refused(reason));
        }

        tx.execute(
            "INSERT INTO public.memory_supersessions (tenant_id, new_assertion_id, old_assertion_id)
             VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
            &[&tenant_id, &new_assertion_id, &old_assertion_id],
        )
        .await
        .context("insert memory_supersessions")?;

        tx.commit().await?;
        Ok(SupersessionOutcome::Recorded)
    }
}

/// Result of `MemoryStore::record_supersession`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupersessionOutcome {
    Recorded,
    Refused(SupersessionRefusal),
}

/// Why a supersession was refused by policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupersessionRefusal {
    SelfLink,
    Missing,
    UntrustedOverTrusted,
    NonProfileOverProfile,
    Cycle,
}

/// True when following `new -> old` supersession links from `from` reaches `to`
/// (so adding `to -> from` would close a cycle).
pub(crate) async fn supersession_reaches(tx: &Transaction<'_>, tenant_id: &str, from: &str, to: &str) -> Result<bool> {
    let hit = tx
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
            &[&tenant_id, &from, &to],
        )
        .await
        .context("walk supersession chain")?;
    Ok(hit.is_some())
}
