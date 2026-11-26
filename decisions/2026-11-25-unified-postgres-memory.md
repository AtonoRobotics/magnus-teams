# Unified Rust + Postgres Memory Engine

Date: 2026-11-25
Status: Accepted (Phase 0/1)

## Decision

Collapse Magnus's fragmented memory and knowledge stores into a single Postgres-backed Rust subsystem under `crates/magnus/src/memory/` with:

1. **Single Postgres + `pgvector >= 0.8.0` (`halfvec(N)`)**:
   - `memory_sources` is FTS-only (`tsvector` + GIN) for instant sync recall and citation storage; hard-deleted on revocation/erasure (no `deleted_at` tombstones).
   - `memory_assertions` holds facts, chunks, lessons, and pinned profile entries (`kind = 'profile'`, <= 8 KB, strictly `!untrusted`) behind a single partial HNSW index (`halfvec(N) WHERE NOT archived`) with `hnsw.iterative_scan = relaxed_order`.
2. **Thin canonical `memory_entities` with zero ACL drift**:
   - `memory_entities` holds only `(tenant_id, entity_id, entity_type, external_id)` with no `readers`, free-text names, `aliases`, or `embedding`; per-edge display names (`src_name`, `dst_name`) live on RLS-protected `memory_edges` to prevent cross-ACL name leakage.
   - RLS on `memory_entities` for `{role}_reader` delegates visibility to `EXISTS` over RLS-filtered `memory_edges`, preventing cross-ACL leakage and stale ACL recomputation.
3. **Strict taint & visibility separation**:
   - `memory_edges` is strictly entity-to-entity (composite FKs to `memory_entities`) and carries both `untrusted` and `private` flags; clean reads filter `!untrusted` on every graph hop.
   - Assertion-to-assertion supersession lives in `memory_supersessions` (composite FKs `ON DELETE CASCADE` to `memory_assertions`), enabling read-time visibility-scoped supersession via a single RLS-filtered `NOT EXISTS` subquery.
4. **Fenced async enrichment**:
   - Async workers claim jobs via `FOR UPDATE SKIP LOCKED` and fence commits with `SELECT 1 FROM memory_sources WHERE (tenant_id, source_id, rev) = ... FOR SHARE` inside the commit transaction, rolling back if the source was hard-deleted mid-flight.
5. **Hard-delete erasure**:
   - Revoking a source hard-deletes `memory_sources`, cascades `memory_evidence`, and deletes zero-evidence derivations as well as any multi-source distilled `lesson`/`profile` that depended on the erased source.

## Rationale

- Eliminates multi-store sync drift, unindexed substring scans, and soft-delete erasure leaks. All reader-facing visibility and tenant isolation (`magnus.principals` + `magnus.tenant`) are database-enforced via forced Postgres RLS on `{role}_reader`; kernel write and maintenance paths run as `{role}` (`USING (true)`, matching `public.knowledge`) scoped by composite `(tenant_id, ...)` foreign keys and explicit `tenant_id` predicates.
- Keeps rollback zero-risk via `MemoryMode` (`legacy | shadow | v2` + per-person `person_modes`).
