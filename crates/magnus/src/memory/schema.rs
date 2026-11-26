//! Additive Postgres DDL for the unified memory engine.
//!
//! Creates all seven tenant-scoped tables with composite `(tenant_id, ...)` foreign
//! keys, GIN full-text indexes, covering graph indexes, and forced Postgres RLS
//! policies enforced for `{role}_reader`.

use crate::memory::MemoryEngine;
use anyhow::{Context, Result};

/// Common CNF + tenant RLS predicate used across every memory table.
fn reader_policy_using() -> &'static str {
    "tenant_id = COALESCE(NULLIF(current_setting('magnus.tenant', true), ''), 'default') \
     AND NOT EXISTS ( \
         SELECT 1 FROM jsonb_array_elements(readers) AS clause \
         WHERE NOT (clause ?| ARRAY( \
             SELECT jsonb_array_elements_text( \
                 COALESCE(NULLIF(current_setting('magnus.principals', true), ''), '[]')::jsonb \
             ) \
         )) \
     )"
}

/// Return idempotent DDL creating all unified memory tables, indexes, and forced RLS.
pub fn migrate_sql(role: &str) -> String {
    let reader = format!("{role}_reader");
    let using = reader_policy_using();
    format!(
        "CREATE EXTENSION IF NOT EXISTS vector;
         DO $$ BEGIN CREATE ROLE {reader} NOLOGIN; EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL; END $$;
         DO $$ BEGIN IF NOT pg_has_role('{role}', '{reader}', 'MEMBER') THEN GRANT {reader} TO {role}; END IF; END $$;

         CREATE TABLE IF NOT EXISTS public.memory_sources (
             tenant_id    text NOT NULL DEFAULT 'default',
             source_id    text NOT NULL,
             rev          bigint NOT NULL DEFAULT 1,
             owner_id     text,
             readers      jsonb NOT NULL DEFAULT '[]'::jsonb,
             untrusted    boolean NOT NULL DEFAULT false,
             private      boolean NOT NULL DEFAULT false,
             kind         text NOT NULL DEFAULT 'note',
             content_hash text NOT NULL,
             text         text NOT NULL,
             metadata     jsonb NOT NULL DEFAULT '{{}}'::jsonb,
             tsv          tsvector NOT NULL,
             embedding    vector,
             model        text,
             occurred_at  timestamptz NOT NULL DEFAULT now(),
             created_at   timestamptz NOT NULL DEFAULT now(),
             deleted_at   timestamptz,
             PRIMARY KEY (tenant_id, source_id, rev)
         );
         CREATE INDEX IF NOT EXISTS memory_sources_tsv_idx
             ON public.memory_sources USING GIN (tsv);
         CREATE INDEX IF NOT EXISTS memory_sources_live_idx
             ON public.memory_sources (tenant_id, occurred_at DESC)
             WHERE deleted_at IS NULL;
         CREATE INDEX IF NOT EXISTS memory_sources_hash_idx
             ON public.memory_sources (tenant_id, source_id, content_hash)
             WHERE deleted_at IS NULL;
         CREATE INDEX IF NOT EXISTS memory_sources_owner_idx
             ON public.memory_sources (tenant_id, owner_id)
             WHERE owner_id IS NOT NULL;

         CREATE TABLE IF NOT EXISTS public.memory_assertions (
             tenant_id        text NOT NULL DEFAULT 'default',
             assertion_id     text NOT NULL,
             kind             text NOT NULL DEFAULT 'fact',
             owner_id         text,
             readers          jsonb NOT NULL DEFAULT '[]'::jsonb,
             untrusted        boolean NOT NULL DEFAULT false,
             private          boolean NOT NULL DEFAULT false,
             statement        text NOT NULL,
             valid_from       timestamptz,
             valid_to         timestamptz,
             confidence       double precision NOT NULL DEFAULT 1.0,
             feedback_weight  double precision NOT NULL DEFAULT 1.0,
             access_count     integer NOT NULL DEFAULT 0,
             last_accessed_at timestamptz,
             archived         boolean NOT NULL DEFAULT false,
             stale            boolean NOT NULL DEFAULT false,
             tsv              tsvector NOT NULL,
             embedding        vector,
             model            text,
             created_at       timestamptz NOT NULL DEFAULT now(),
             updated_at       timestamptz NOT NULL DEFAULT now(),
             PRIMARY KEY (tenant_id, assertion_id)
         );
         CREATE INDEX IF NOT EXISTS memory_assertions_tsv_idx
             ON public.memory_assertions USING GIN (tsv);
         CREATE INDEX IF NOT EXISTS memory_assertions_active_idx
             ON public.memory_assertions (tenant_id, kind, created_at DESC)
             WHERE archived = false;
         CREATE INDEX IF NOT EXISTS memory_assertions_temporal_idx
             ON public.memory_assertions (tenant_id, valid_from, valid_to)
             WHERE archived = false AND (valid_from IS NOT NULL OR valid_to IS NOT NULL);
         CREATE INDEX IF NOT EXISTS memory_assertions_owner_idx
             ON public.memory_assertions (tenant_id, owner_id)
             WHERE owner_id IS NOT NULL;

         CREATE TABLE IF NOT EXISTS public.memory_entities (
             tenant_id      text NOT NULL DEFAULT 'default',
             entity_id      text NOT NULL,
             entity_type    text NOT NULL,
             canonical_name text NOT NULL,
             norm_name      text NOT NULL,
             external_id    text,
             aliases        jsonb NOT NULL DEFAULT '[]'::jsonb,
             readers        jsonb NOT NULL DEFAULT '[]'::jsonb,
             embedding      vector,
             model          text,
             created_at     timestamptz NOT NULL DEFAULT now(),
             updated_at     timestamptz NOT NULL DEFAULT now(),
             PRIMARY KEY (tenant_id, entity_id)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS memory_entities_ext_idx
             ON public.memory_entities (tenant_id, entity_type, external_id)
             WHERE external_id IS NOT NULL;
         CREATE INDEX IF NOT EXISTS memory_entities_norm_idx
             ON public.memory_entities (tenant_id, norm_name);

         CREATE TABLE IF NOT EXISTS public.memory_edges (
             tenant_id  text NOT NULL DEFAULT 'default',
             edge_id    text NOT NULL,
             src_id     text NOT NULL,
             dst_id     text NOT NULL,
             rel_type   text NOT NULL,
             readers    jsonb NOT NULL DEFAULT '[]'::jsonb,
             untrusted  boolean NOT NULL DEFAULT false,
             valid_from timestamptz,
             valid_to   timestamptz,
             properties jsonb NOT NULL DEFAULT '{{}}'::jsonb,
             created_at timestamptz NOT NULL DEFAULT now(),
             PRIMARY KEY (tenant_id, edge_id)
         );
         CREATE INDEX IF NOT EXISTS memory_edges_src_cover_idx
             ON public.memory_edges (tenant_id, src_id)
             INCLUDE (dst_id, rel_type, untrusted);
         CREATE INDEX IF NOT EXISTS memory_edges_dst_cover_idx
             ON public.memory_edges (tenant_id, dst_id)
             INCLUDE (src_id, rel_type, untrusted);
         CREATE INDEX IF NOT EXISTS memory_edges_rel_idx
             ON public.memory_edges (tenant_id, rel_type, dst_id);

         CREATE TABLE IF NOT EXISTS public.memory_evidence (
             tenant_id   text NOT NULL DEFAULT 'default',
             target_kind text NOT NULL,
             target_id   text NOT NULL,
             source_id   text NOT NULL,
             source_rev  bigint NOT NULL,
             created_at  timestamptz NOT NULL DEFAULT now(),
             PRIMARY KEY (tenant_id, target_kind, target_id, source_id, source_rev),
             FOREIGN KEY (tenant_id, source_id, source_rev)
                 REFERENCES public.memory_sources (tenant_id, source_id, rev)
                 ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS memory_evidence_source_idx
             ON public.memory_evidence (tenant_id, source_id, source_rev);
         CREATE INDEX IF NOT EXISTS memory_evidence_target_idx
             ON public.memory_evidence (tenant_id, target_kind, target_id);

         CREATE TABLE IF NOT EXISTS public.memory_enrichment_jobs (
             tenant_id   text NOT NULL DEFAULT 'default',
             job_id      bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
             source_id   text NOT NULL,
             source_rev  bigint NOT NULL,
             status      text NOT NULL DEFAULT 'pending',
             attempts    integer NOT NULL DEFAULT 0,
             next_run_at timestamptz NOT NULL DEFAULT now(),
             last_error  text,
             created_at  timestamptz NOT NULL DEFAULT now(),
             updated_at  timestamptz NOT NULL DEFAULT now(),
             FOREIGN KEY (tenant_id, source_id, source_rev)
                 REFERENCES public.memory_sources (tenant_id, source_id, rev)
                 ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS memory_enrichment_jobs_ready_idx
             ON public.memory_enrichment_jobs (status, next_run_at)
             WHERE status = 'pending';

         CREATE TABLE IF NOT EXISTS public.memory_profile (
             tenant_id  text NOT NULL DEFAULT 'default',
             person     text NOT NULL,
             readers    jsonb NOT NULL,
             body       text NOT NULL,
             updated_at timestamptz NOT NULL DEFAULT now(),
             PRIMARY KEY (tenant_id, person)
         );

         CREATE TABLE IF NOT EXISTS public.memory_curator_state (
             tenant_id      text NOT NULL DEFAULT 'default',
             stage          text NOT NULL,
             watermark_at   timestamptz NOT NULL DEFAULT to_timestamp(0),
             last_run_at    timestamptz,
             PRIMARY KEY (tenant_id, stage)
         );

         REVOKE ALL ON public.memory_sources, public.memory_assertions, public.memory_entities,
             public.memory_edges, public.memory_evidence, public.memory_enrichment_jobs,
             public.memory_profile, public.memory_curator_state FROM PUBLIC;
         GRANT SELECT, INSERT, UPDATE, DELETE ON public.memory_sources, public.memory_assertions,
             public.memory_entities, public.memory_edges, public.memory_evidence,
             public.memory_enrichment_jobs, public.memory_profile, public.memory_curator_state TO {role};
         GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO {role};
         GRANT USAGE ON SCHEMA public TO {reader};
         GRANT SELECT ON public.memory_sources, public.memory_assertions, public.memory_entities,
             public.memory_edges, public.memory_evidence, public.memory_profile TO {reader};

         ALTER TABLE public.memory_sources ENABLE ROW LEVEL SECURITY;
         ALTER TABLE public.memory_sources FORCE ROW LEVEL SECURITY;
         DROP POLICY IF EXISTS memory_sources_kernel ON public.memory_sources;
         CREATE POLICY memory_sources_kernel ON public.memory_sources FOR ALL TO {role} USING (true) WITH CHECK (true);
         DROP POLICY IF EXISTS memory_sources_read ON public.memory_sources;
         CREATE POLICY memory_sources_read ON public.memory_sources FOR SELECT TO {reader}
             USING (deleted_at IS NULL AND {using});

         ALTER TABLE public.memory_assertions ENABLE ROW LEVEL SECURITY;
         ALTER TABLE public.memory_assertions FORCE ROW LEVEL SECURITY;
         DROP POLICY IF EXISTS memory_assertions_kernel ON public.memory_assertions;
         CREATE POLICY memory_assertions_kernel ON public.memory_assertions FOR ALL TO {role} USING (true) WITH CHECK (true);
         DROP POLICY IF EXISTS memory_assertions_read ON public.memory_assertions;
         CREATE POLICY memory_assertions_read ON public.memory_assertions FOR SELECT TO {reader}
             USING (archived = false AND {using});

         ALTER TABLE public.memory_entities ENABLE ROW LEVEL SECURITY;
         ALTER TABLE public.memory_entities FORCE ROW LEVEL SECURITY;
         DROP POLICY IF EXISTS memory_entities_kernel ON public.memory_entities;
         CREATE POLICY memory_entities_kernel ON public.memory_entities FOR ALL TO {role} USING (true) WITH CHECK (true);
         DROP POLICY IF EXISTS memory_entities_read ON public.memory_entities;
         CREATE POLICY memory_entities_read ON public.memory_entities FOR SELECT TO {reader}
             USING ({using});

         ALTER TABLE public.memory_edges ENABLE ROW LEVEL SECURITY;
         ALTER TABLE public.memory_edges FORCE ROW LEVEL SECURITY;
         DROP POLICY IF EXISTS memory_edges_kernel ON public.memory_edges;
         CREATE POLICY memory_edges_kernel ON public.memory_edges FOR ALL TO {role} USING (true) WITH CHECK (true);
         DROP POLICY IF EXISTS memory_edges_read ON public.memory_edges;
         CREATE POLICY memory_edges_read ON public.memory_edges FOR SELECT TO {reader}
             USING ({using});

         ALTER TABLE public.memory_evidence ENABLE ROW LEVEL SECURITY;
         ALTER TABLE public.memory_evidence FORCE ROW LEVEL SECURITY;
         DROP POLICY IF EXISTS memory_evidence_kernel ON public.memory_evidence;
         CREATE POLICY memory_evidence_kernel ON public.memory_evidence FOR ALL TO {role} USING (true) WITH CHECK (true);
         DROP POLICY IF EXISTS memory_evidence_read ON public.memory_evidence;
         CREATE POLICY memory_evidence_read ON public.memory_evidence FOR SELECT TO {reader}
             USING (
                 tenant_id = COALESCE(NULLIF(current_setting('magnus.tenant', true), ''), 'default')
                 AND EXISTS (
                     SELECT 1 FROM public.memory_sources s
                     WHERE s.tenant_id = memory_evidence.tenant_id
                       AND s.source_id = memory_evidence.source_id
                       AND s.rev = memory_evidence.source_rev
                 )
             );

         ALTER TABLE public.memory_profile ENABLE ROW LEVEL SECURITY;
         ALTER TABLE public.memory_profile FORCE ROW LEVEL SECURITY;
         DROP POLICY IF EXISTS memory_profile_kernel ON public.memory_profile;
         CREATE POLICY memory_profile_kernel ON public.memory_profile FOR ALL TO {role} USING (true) WITH CHECK (true);
         DROP POLICY IF EXISTS memory_profile_read ON public.memory_profile;
         CREATE POLICY memory_profile_read ON public.memory_profile FOR SELECT TO {reader}
             USING ({using});"
    )
}

impl MemoryEngine {
    /// Ensure dimension-specific partial HNSW indexes exist for the active embedding
    /// dimension so untyped `vector` columns still get full HNSW + iterative scan support.
    pub async fn ensure_hnsw_indexes(&self, dims: usize) -> Result<()> {
        anyhow::ensure!(dims > 0 && dims <= 4096, "invalid vector dimension {dims}");
        let mut guard = self.client().await?;
        let client = guard.as_mut().context("memory engine connection")?;
        let sql = format!(
            "CREATE INDEX IF NOT EXISTS memory_sources_hnsw_{dims}_idx
                 ON public.memory_sources
                 USING hnsw ((embedding::vector({dims})) vector_cosine_ops)
                 WHERE embedding IS NOT NULL AND vector_dims(embedding) = {dims};
             CREATE INDEX IF NOT EXISTS memory_assertions_hnsw_{dims}_idx
                 ON public.memory_assertions
                 USING hnsw ((embedding::vector({dims})) vector_cosine_ops)
                 WHERE embedding IS NOT NULL AND vector_dims(embedding) = {dims};"
        );
        client.batch_execute(&sql).await.context("create dimension-specific HNSW indexes")?;
        Ok(())
    }
}
