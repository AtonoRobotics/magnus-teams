//! Additive Postgres DDL for the unified memory engine (`pgvector >= 0.8.0`).
//! Mode-gated via `migrate_if_enabled` so `MemoryMode::Legacy` stays zero-risk.

use crate::memory_index::{MemoryConfig, PROFILE_MAX_BYTES};
use anyhow::{Context, Result};
use semver::Version;

/// HNSW graph degree parameter `m` for `memory_assertions.embedding`.
pub const HNSW_M: usize = 16;
/// HNSW build-time candidate list size `ef_construction`.
pub const HNSW_EF_CONSTRUCTION: usize = 64;
/// Minimum `pgvector` version required for `halfvec(N)` + `hnsw.iterative_scan`.
pub const MIN_PGVECTOR_MAJOR: u64 = 0;
pub const MIN_PGVECTOR_MINOR: u64 = 8;
pub const MIN_PGVECTOR_PATCH: u64 = 0;
/// Postgres transaction-scoped advisory lock key serializing concurrent memory migrations.
const MIGRATE_ADVISORY_LOCK_KEY: i64 = 0x4D45_4D5F_4D49_4752;
/// Monotonic schema version recorded in `public.memory_schema_meta` so routine
/// restarts skip DDL and take zero table locks.
/// Version 2 added `memory_curator_watermarks`; version 3 replaces it with the
/// per-assertion `collision_pending` flag and drops the table.
pub const MEMORY_SCHEMA_VERSION: i32 = 3;

/// Apply unified memory migrations only when `cfg.is_unified_enabled()` is true.
/// Verifies `cfg.check()` and `pgvector >= 0.8.0` before running DDL with the
/// configured `cfg.embedding_dim`.
pub async fn migrate_if_enabled(admin_url: &str, kernel_url: &str, cfg: &MemoryConfig) -> Result<()> {
    cfg.check()?;
    if !cfg.is_unified_enabled() {
        return Ok(());
    }
    let parsed = url::Url::parse(kernel_url).context("parse kernel database url")?;
    let role = crate::log::ident(parsed.username())?;
    let mut admin = crate::log::connect(admin_url).await?;
    let tx = admin.transaction().await.context("begin memory migration transaction")?;
    tx.execute("SELECT pg_advisory_xact_lock($1)", &[&MIGRATE_ADVISORY_LOCK_KEY]).await.context("acquire memory migration advisory lock")?;

    // Fast path: if schema is already at MEMORY_SCHEMA_VERSION, verify embedding_dim
    // and return without executing any ALTER TABLE (zero table locks on restarts).
    let meta_exists: bool = tx.query_one("SELECT to_regclass('public.memory_schema_meta') IS NOT NULL", &[]).await?.get(0);
    if meta_exists {
        if let Some(meta) = tx.query_opt("SELECT version, embedding_dim FROM public.memory_schema_meta WHERE id = 1", &[]).await? {
            let ver: i32 = meta.get(0);
            let dim: i32 = meta.get(1);
            anyhow::ensure!(dim as usize == cfg.embedding_dim, "configured [memory] embedding_dim ({}) does not match existing database schema ({dim})", cfg.embedding_dim);
            if ver >= MEMORY_SCHEMA_VERSION {
                tx.commit().await.context("commit memory migration fast-path")?;
                return Ok(());
            }
        }
    }

    tx.batch_execute("CREATE EXTENSION IF NOT EXISTS vector;").await.context("enable pgvector extension")?;
    let row = tx.query_one("SELECT extversion FROM pg_extension WHERE extname = 'vector'", &[]).await.context("check pgvector extension version")?;
    let ext_ver: String = row.get(0);
    check_pgvector_version(&ext_ver)?;
    tx.batch_execute(&migrate_sql(role, cfg.embedding_dim)).await.context("apply unified memory schema migration")?;
    tx.execute(
        "CREATE TABLE IF NOT EXISTS public.memory_schema_meta (
             id            INTEGER PRIMARY KEY CHECK (id = 1),
             version       INTEGER NOT NULL,
             embedding_dim INTEGER NOT NULL,
             updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
         )",
        &[],
    )
    .await?;
    let dim_i32 = i32::try_from(cfg.embedding_dim).context("embedding_dim fits in i32")?;
    tx.execute(
        "INSERT INTO public.memory_schema_meta (id, version, embedding_dim)
         VALUES (1, $1, $2)
         ON CONFLICT (id) DO UPDATE SET version = EXCLUDED.version, embedding_dim = EXCLUDED.embedding_dim, updated_at = now()",
        &[&MEMORY_SCHEMA_VERSION, &dim_i32],
    )
    .await?;
    tx.commit().await.context("commit memory migration transaction")?;
    Ok(())
}

/// Ensure an installed `pgvector` version string satisfies `>= 0.8.0`.
pub fn check_pgvector_version(ext_ver: &str) -> Result<()> {
    let parsed = Version::parse(ext_ver)
        .or_else(|_| Version::parse(&format!("{ext_ver}.0")))
        .with_context(|| format!("unparseable pgvector version {ext_ver:?}; require >= {MIN_PGVECTOR_MAJOR}.{MIN_PGVECTOR_MINOR}.{MIN_PGVECTOR_PATCH}"))?;
    let min = Version::new(MIN_PGVECTOR_MAJOR, MIN_PGVECTOR_MINOR, MIN_PGVECTOR_PATCH);
    anyhow::ensure!(parsed >= min, "pgvector {parsed} is too old for unified memory (requires >= {min} for halfvec + hnsw.iterative_scan)");
    Ok(())
}

/// Build an idempotent `CREATE POLICY` block that avoids `DROP POLICY` lock churn on restarts.
fn ensure_policy_sql(table: &str, name: &str, body: &str) -> String {
    format!(
        "DO $$ BEGIN \
             IF NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname = 'public' AND tablename = '{table}' AND policyname = '{name}') THEN \
                 CREATE POLICY {name} ON public.{table} {body}; \
             END IF; \
         END $$;"
    )
}

/// Return idempotent Postgres DDL creating all unified memory tables, composite
/// tenant foreign keys, partial `halfvec(N)` HNSW index, and forced tenant + CNF RLS policies.
pub fn migrate_sql(role: &str, embedding_dim: usize) -> String {
    let reader = format!("{role}_reader");
    let tenant_check = "tenant_id = current_setting('magnus.tenant', true)";
    let cnf_check = "\
        tenant_id = current_setting('magnus.tenant', true) AND \
        NOT EXISTS ( \
            SELECT 1 FROM jsonb_array_elements(readers) AS clause \
            WHERE NOT (clause ?| ARRAY( \
                SELECT jsonb_array_elements_text( \
                    COALESCE(NULLIF(current_setting('magnus.principals', true), ''), '[]')::jsonb \
                ) \
            )) \
        )";

    let pol_sources_k = ensure_policy_sql("memory_sources", "memory_sources_kernel", &format!("FOR ALL TO {role} USING (true) WITH CHECK (true)"));
    let pol_sources_r = ensure_policy_sql("memory_sources", "memory_sources_read", &format!("FOR SELECT TO {reader} USING ({cnf_check})"));
    let pol_assertions_k = ensure_policy_sql("memory_assertions", "memory_assertions_kernel", &format!("FOR ALL TO {role} USING (true) WITH CHECK (true)"));
    let pol_assertions_r = ensure_policy_sql("memory_assertions", "memory_assertions_read", &format!("FOR SELECT TO {reader} USING ({cnf_check})"));
    let pol_entities_k = ensure_policy_sql("memory_entities", "memory_entities_kernel", &format!("FOR ALL TO {role} USING (true) WITH CHECK (true)"));
    let pol_entities_r = ensure_policy_sql(
        "memory_entities",
        "memory_entities_read",
        &format!(
            "FOR SELECT TO {reader} USING ({tenant_check} AND EXISTS ( \
                 SELECT 1 FROM public.memory_edges e \
                 WHERE e.tenant_id = memory_entities.tenant_id \
                   AND (e.src_id = memory_entities.entity_id OR e.dst_id = memory_entities.entity_id) \
             ))"
        ),
    );
    let pol_edges_k = ensure_policy_sql("memory_edges", "memory_edges_kernel", &format!("FOR ALL TO {role} USING (true) WITH CHECK (true)"));
    let pol_edges_r = ensure_policy_sql("memory_edges", "memory_edges_read", &format!("FOR SELECT TO {reader} USING ({cnf_check})"));
    let pol_sup_k = ensure_policy_sql("memory_supersessions", "memory_supersessions_kernel", &format!("FOR ALL TO {role} USING (true) WITH CHECK (true)"));
    let pol_sup_r = ensure_policy_sql(
        "memory_supersessions",
        "memory_supersessions_read",
        &format!(
            "FOR SELECT TO {reader} USING ({tenant_check} AND EXISTS ( \
                 SELECT 1 FROM public.memory_assertions a \
                 WHERE a.tenant_id = memory_supersessions.tenant_id \
                   AND a.assertion_id = memory_supersessions.new_assertion_id \
             ))"
        ),
    );
    let pol_ev_k = ensure_policy_sql("memory_evidence", "memory_evidence_kernel", &format!("FOR ALL TO {role} USING (true) WITH CHECK (true)"));
    let pol_ev_r = ensure_policy_sql(
        "memory_evidence",
        "memory_evidence_read",
        &format!(
            "FOR SELECT TO {reader} USING ({tenant_check} AND EXISTS ( \
                 SELECT 1 FROM public.memory_sources s \
                 WHERE s.tenant_id = memory_evidence.tenant_id \
                   AND s.source_id = memory_evidence.source_id \
                   AND s.rev = memory_evidence.source_rev \
             ))"
        ),
    );
    let pol_jobs_k = ensure_policy_sql("memory_enrichment_jobs", "memory_enrichment_jobs_kernel", &format!("FOR ALL TO {role} USING (true) WITH CHECK (true)"));

    format!(
        "DO $$ BEGIN CREATE ROLE {reader} NOLOGIN; EXCEPTION WHEN duplicate_object THEN NULL; END $$;
         DO $$ BEGIN IF NOT pg_has_role('{role}', '{reader}', 'MEMBER') THEN GRANT {reader} TO {role}; END IF; END $$;

         -- 1. memory_sources: FTS-only citation & raw source store (hard-deleted on erasure).
         CREATE TABLE IF NOT EXISTS public.memory_sources (
             tenant_id    TEXT NOT NULL,
             source_id    TEXT NOT NULL,
             rev          BIGINT NOT NULL,
             owner_id     TEXT NOT NULL,
             readers      JSONB NOT NULL,
             untrusted    BOOLEAN NOT NULL DEFAULT FALSE,
             private      BOOLEAN NOT NULL DEFAULT FALSE,
             content_hash TEXT NOT NULL,
             text         TEXT NOT NULL,
             tsv          TSVECTOR NOT NULL,
             occurred_at  TIMESTAMPTZ NOT NULL,
             created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
             PRIMARY KEY (tenant_id, source_id, rev)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS memory_sources_exact_dedup_idx
             ON public.memory_sources (tenant_id, source_id, content_hash, readers, untrusted, private);
         CREATE INDEX IF NOT EXISTS memory_sources_tsv_idx
             ON public.memory_sources USING GIN (tsv);
         CREATE INDEX IF NOT EXISTS memory_sources_occurred_idx
             ON public.memory_sources (tenant_id, occurred_at DESC);
         ALTER TABLE public.memory_sources ENABLE ROW LEVEL SECURITY;
         ALTER TABLE public.memory_sources FORCE ROW LEVEL SECURITY;
         REVOKE ALL ON public.memory_sources FROM PUBLIC;
         GRANT SELECT, INSERT, UPDATE, DELETE ON public.memory_sources TO {role};
         GRANT USAGE ON SCHEMA public TO {reader};
         GRANT SELECT ON public.memory_sources TO {reader};
         {pol_sources_k}
         {pol_sources_r}

         -- 2. memory_assertions: facts, chunks, lessons, and pinned profile (kind = 'profile').
         CREATE TABLE IF NOT EXISTS public.memory_assertions (
             tenant_id        TEXT NOT NULL,
             assertion_id     TEXT NOT NULL,
             kind             TEXT NOT NULL,
             owner_id         TEXT NOT NULL,
             readers          JSONB NOT NULL,
             untrusted        BOOLEAN NOT NULL DEFAULT FALSE,
             private          BOOLEAN NOT NULL DEFAULT FALSE,
             statement        TEXT NOT NULL,
             valid_from       TIMESTAMPTZ NOT NULL,
             valid_to         TIMESTAMPTZ,
             confidence       REAL NOT NULL DEFAULT 1.0,
             access_count     INTEGER NOT NULL DEFAULT 0,
             last_accessed_at TIMESTAMPTZ,
             archived         BOOLEAN NOT NULL DEFAULT FALSE,
             tsv              TSVECTOR NOT NULL,
             embedding        halfvec({embedding_dim}),
             created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
             collision_pending BOOLEAN NOT NULL DEFAULT TRUE,
             PRIMARY KEY (tenant_id, assertion_id),
             CHECK (kind IN ('fact', 'chunk', 'lesson', 'profile')),
             CHECK (kind <> 'profile' OR NOT untrusted),
             CHECK (kind <> 'profile' OR octet_length(statement) <= {PROFILE_MAX_BYTES})
         );
         CREATE INDEX IF NOT EXISTS memory_assertions_tsv_idx
             ON public.memory_assertions USING GIN (tsv);
         CREATE INDEX IF NOT EXISTS memory_assertions_hnsw_idx
             ON public.memory_assertions USING hnsw (embedding halfvec_cosine_ops)
             WITH (m = {HNSW_M}, ef_construction = {HNSW_EF_CONSTRUCTION})
             WHERE NOT archived;
         CREATE INDEX IF NOT EXISTS memory_assertions_active_idx
             ON public.memory_assertions (tenant_id, kind, valid_from DESC)
             WHERE NOT archived;
         -- Collision review queue: an assertion is reviewed once it is
         -- committed, however long its writing transaction ran (version 3).
         ALTER TABLE public.memory_assertions ADD COLUMN IF NOT EXISTS collision_pending BOOLEAN NOT NULL DEFAULT TRUE;
         CREATE INDEX IF NOT EXISTS memory_assertions_collision_pending_idx
             ON public.memory_assertions (tenant_id, created_at, assertion_id)
             WHERE collision_pending AND NOT archived AND embedding IS NOT NULL;
         ALTER TABLE public.memory_assertions ENABLE ROW LEVEL SECURITY;
         ALTER TABLE public.memory_assertions FORCE ROW LEVEL SECURITY;
         REVOKE ALL ON public.memory_assertions FROM PUBLIC;
         GRANT SELECT, INSERT, UPDATE, DELETE ON public.memory_assertions TO {role};
         GRANT SELECT ON public.memory_assertions TO {reader};
         {pol_assertions_k}
         {pol_assertions_r}

         -- 3. memory_entities: thin canonical anchors (no readers, free-text names, aliases, or embedding).
         CREATE TABLE IF NOT EXISTS public.memory_entities (
             tenant_id   TEXT NOT NULL,
             entity_id   TEXT NOT NULL,
             entity_type TEXT NOT NULL,
             external_id TEXT,
             PRIMARY KEY (tenant_id, entity_id)
         );
         CREATE INDEX IF NOT EXISTS memory_entities_ext_idx
             ON public.memory_entities (tenant_id, lower(external_id))
             WHERE external_id IS NOT NULL;
         ALTER TABLE public.memory_entities ENABLE ROW LEVEL SECURITY;
         ALTER TABLE public.memory_entities FORCE ROW LEVEL SECURITY;
         REVOKE ALL ON public.memory_entities FROM PUBLIC;
         GRANT SELECT, INSERT, UPDATE, DELETE ON public.memory_entities TO {role};
         GRANT SELECT ON public.memory_entities TO {reader};
         {pol_entities_k}

         -- 4. memory_edges: entity-to-entity graph edges carrying per-edge display names + taint.
         CREATE TABLE IF NOT EXISTS public.memory_edges (
             tenant_id  TEXT NOT NULL,
             edge_id    TEXT NOT NULL,
             src_id     TEXT NOT NULL,
             dst_id     TEXT NOT NULL,
             src_name   TEXT NOT NULL,
             dst_name   TEXT NOT NULL,
             rel_type   TEXT NOT NULL,
             readers    JSONB NOT NULL,
             untrusted  BOOLEAN NOT NULL DEFAULT FALSE,
             private    BOOLEAN NOT NULL DEFAULT FALSE,
             valid_from TIMESTAMPTZ NOT NULL,
             valid_to   TIMESTAMPTZ,
             properties JSONB NOT NULL DEFAULT '{{}}'::jsonb,
             PRIMARY KEY (tenant_id, edge_id),
             FOREIGN KEY (tenant_id, src_id) REFERENCES public.memory_entities (tenant_id, entity_id) ON DELETE CASCADE,
             FOREIGN KEY (tenant_id, dst_id) REFERENCES public.memory_entities (tenant_id, entity_id) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS memory_edges_src_idx
             ON public.memory_edges (tenant_id, src_id, dst_id);
         CREATE INDEX IF NOT EXISTS memory_edges_dst_idx
             ON public.memory_edges (tenant_id, dst_id, src_id);
         CREATE INDEX IF NOT EXISTS memory_edges_src_name_idx
             ON public.memory_edges (tenant_id, lower(src_name));
         CREATE INDEX IF NOT EXISTS memory_edges_dst_name_idx
             ON public.memory_edges (tenant_id, lower(dst_name));
         ALTER TABLE public.memory_edges ENABLE ROW LEVEL SECURITY;
         ALTER TABLE public.memory_edges FORCE ROW LEVEL SECURITY;
         REVOKE ALL ON public.memory_edges FROM PUBLIC;
         GRANT SELECT, INSERT, UPDATE, DELETE ON public.memory_edges TO {role};
         GRANT SELECT ON public.memory_edges TO {reader};
         {pol_edges_k}
         {pol_edges_r}
         {pol_entities_r}

         -- 5. memory_supersessions: assertion-to-assertion supersession with composite FKs.
         CREATE TABLE IF NOT EXISTS public.memory_supersessions (
             tenant_id        TEXT NOT NULL,
             new_assertion_id TEXT NOT NULL,
             old_assertion_id TEXT NOT NULL,
             created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
             PRIMARY KEY (tenant_id, new_assertion_id, old_assertion_id),
             FOREIGN KEY (tenant_id, new_assertion_id) REFERENCES public.memory_assertions (tenant_id, assertion_id) ON DELETE CASCADE,
             FOREIGN KEY (tenant_id, old_assertion_id) REFERENCES public.memory_assertions (tenant_id, assertion_id) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS memory_supersessions_old_idx
             ON public.memory_supersessions (tenant_id, old_assertion_id);
         ALTER TABLE public.memory_supersessions ENABLE ROW LEVEL SECURITY;
         ALTER TABLE public.memory_supersessions FORCE ROW LEVEL SECURITY;
         REVOKE ALL ON public.memory_supersessions FROM PUBLIC;
         GRANT SELECT, INSERT, UPDATE, DELETE ON public.memory_supersessions TO {role};
         GRANT SELECT ON public.memory_supersessions TO {reader};
         {pol_sup_k}
         {pol_sup_r}

         -- 6. memory_evidence: provenance link from assertions/edges to versioned sources.
         CREATE TABLE IF NOT EXISTS public.memory_evidence (
             tenant_id   TEXT NOT NULL,
             target_kind TEXT NOT NULL,
             target_id   TEXT NOT NULL,
             source_id   TEXT NOT NULL,
             source_rev  BIGINT NOT NULL,
             PRIMARY KEY (tenant_id, target_kind, target_id, source_id, source_rev),
             CHECK (target_kind IN ('assertion', 'edge')),
             FOREIGN KEY (tenant_id, source_id, source_rev) REFERENCES public.memory_sources (tenant_id, source_id, rev) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS memory_evidence_target_idx
             ON public.memory_evidence (tenant_id, target_kind, target_id);
         ALTER TABLE public.memory_evidence ENABLE ROW LEVEL SECURITY;
         ALTER TABLE public.memory_evidence FORCE ROW LEVEL SECURITY;
         REVOKE ALL ON public.memory_evidence FROM PUBLIC;
         GRANT SELECT, INSERT, UPDATE, DELETE ON public.memory_evidence TO {role};
         GRANT SELECT ON public.memory_evidence TO {reader};
         {pol_ev_k}
         {pol_ev_r}

         -- 7. memory_enrichment_jobs: durable Postgres queue with lease & forced RLS.
         CREATE TABLE IF NOT EXISTS public.memory_enrichment_jobs (
             tenant_id    TEXT NOT NULL,
             job_id       TEXT NOT NULL,
             source_id    TEXT NOT NULL,
             source_rev   BIGINT NOT NULL,
             status       TEXT NOT NULL DEFAULT 'pending',
             attempts     INTEGER NOT NULL DEFAULT 0,
             next_run_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
             locked_until TIMESTAMPTZ,
             last_error   TEXT,
             created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
             PRIMARY KEY (tenant_id, job_id),
             CHECK (status IN ('pending', 'running', 'done', 'failed')),
             FOREIGN KEY (tenant_id, source_id, source_rev) REFERENCES public.memory_sources (tenant_id, source_id, rev) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS memory_enrichment_jobs_ready_idx
             ON public.memory_enrichment_jobs (next_run_at, job_id)
             WHERE status = 'pending';
         CREATE INDEX IF NOT EXISTS memory_enrichment_jobs_lease_idx
             ON public.memory_enrichment_jobs (locked_until, job_id)
             WHERE status = 'running';
         ALTER TABLE public.memory_enrichment_jobs ENABLE ROW LEVEL SECURITY;
         ALTER TABLE public.memory_enrichment_jobs FORCE ROW LEVEL SECURITY;
         REVOKE ALL ON public.memory_enrichment_jobs FROM PUBLIC;
         GRANT SELECT, INSERT, UPDATE, DELETE ON public.memory_enrichment_jobs TO {role};
         {pol_jobs_k}

         -- 8. memory_curator_watermarks (version 2) is replaced by
         --    memory_assertions.collision_pending; its rows were only progress.
         DROP TABLE IF EXISTS public.memory_curator_watermarks;"
    )
}
