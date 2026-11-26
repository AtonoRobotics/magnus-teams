# Unified Postgres memory engine

Date: 2026-11-26

## Decision

Replace the six fragmented memory and knowledge search paths (personal facts in-RAM prefix scan, unwired `memory_index`/`world_map` tables, fallback-union company KB search, unindexed substring scans on team/bundle search, and unscheduled `memory_curator` stubs) with one unified Rust + Postgres memory engine under `crates/magnus/src/memory/`:

1. **Seven tenant-scoped Postgres tables** (`memory_sources`, `memory_assertions`, `memory_entities`, `memory_edges`, `memory_evidence`, `memory_enrichment_jobs`, `memory_profile`) with composite `(tenant_id, ...)` foreign keys and forced Postgres RLS (`{role}_reader` enforcing both `magnus.tenant` and CNF `readers` against `magnus.principals`).
2. **Evidence separated from derived assertions and edges**: raw sources are immutable and versioned (`source_id`, `rev`), while assertions, entities, edges, and distilled lessons link back to exact `(tenant_id, source_id, rev)` rows through `memory_evidence`.
3. **Read-time visibility-scoped supersession**: there is no global `superseded_by` column. Supersession and contradiction are stored as `memory_edges` (`rel_type = 'supersedes' | 'contradicts'`) subject to the same RLS policy and resolved at read time over the rows visible to the current caller. A private email can override a shared supplier record for its owner without hiding the shared assertion from teammates or leaking the email's existence.
4. **Sync durable commit + fenced async enrichment**: writes commit `memory_sources` + `tsvector` + a durable enrichment job in one transaction (immediately searchable via FTS). Async enrichment embeds and extracts assertions/entities/edges, inherits the strictest `readers` intersection and highest `untrusted`/`private` taint bits, and commits only if every input `(tenant_id, source_id, rev)` is still live (`deleted_at IS NULL`).
5. **Unified hybrid + temporal + graph retrieval**: one SQL-backed API combining exact ID/code precedence, `tsvector` (`ts_rank_cd`) + `pgvector` iterative HNSW (`hnsw.iterative_scan`) fused with RRF, deterministic temporal candidate expansion, and bounded 1–2 hop RLS-filtered SQL graph expansion with zero generation calls on read.
6. **Gated 5-job improvement curator**: feedback EMA weighting (0 LLM, untrusted runs cannot boost clean items), semantic deduplication within identical `(readers, untrusted)` classes unioning provenance, incremental conflict/supersession review, bounded lesson distillation from failed-then-recovered or user-corrected runs, and 30-day cold archival of unused untrusted assertions.
7. **Atomic revocation and erasure**: revoking a source or erasing a person deletes `memory_sources` / `memory_evidence` and atomically removes any assertion, edge, or profile entry with zero surviving evidence in a single transaction, marking multi-source lessons stale for rebuild.

## Why

- Magnus's existing memory layer had strong security/erasure invariants but weak recall (in-RAM 2/3-prefix matching, no personal embeddings, no entity/temporal links, no active consolidation, 6 disconnected stores).
- Adopting an external Python tri-store (e.g., Cognee) would introduce a Python sidecar, lose row-level mixed-visibility authorization on shared entities (dataset-level isolation cannot express a private override on a shared entity), and break taint isolation.
- Keeping everything in Postgres with `pgvector` 0.8+ iterative HNSW, `tsvector`, and two graph tables preserves single-database ACID semantics, forced RLS, taint tracking, and erasure while delivering hybrid + temporal + graph recall and an active improvement loop.
- Existing `public.events` and `public.content` remain untouched; `MemoryMode` (`Legacy | Shadow | V2`) provides instant rollback.
