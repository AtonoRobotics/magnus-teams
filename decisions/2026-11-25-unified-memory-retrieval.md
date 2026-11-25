# Unified Memory Retrieval & Tenant RLS Policy

Date: 2026-11-25
Status: Accepted (Phase 2)

## Decisions

1. **Database-enforced tenant isolation (`magnus.tenant`)**:
   - Every `{role}_reader` RLS policy across `memory_sources`, `memory_assertions`, `memory_edges`, `memory_entities`, `memory_supersessions`, and `memory_evidence` requires `tenant_id = current_setting('magnus.tenant', true)` in addition to CNF `readers` checks.
   - `MemoryStore::begin_reader_tx` binds both `magnus.tenant` and `magnus.principals` transaction-locally, so a caller bug or shared principal name across tenants cannot leak data.

2. **Hybrid RRF + exact distance ordering + exact-match boost**:
   - FTS (`ts_rank_cd`) and `pgvector` HNSW (`halfvec_cosine_ops` with `hnsw.iterative_scan = relaxed_order`, re-sorted by exact distance before assigning `vec_rank`) are fused via `memory_index::hybrid_score` (`RRF_K = 60`, exponential 30-day recency half-life, capped access boost `0.15`, `TIER2_WEIGHT = 0.65`).
   - Exact ID/code/phrase matches receive `EXACT_MATCH_BOOST = 1.5` so literal identifiers outrank paraphrases.
   - `retrieve()` is strictly read-only; callers that want to increment `access_count` invoke `MemoryStore::record_access` explicitly.

3. **Read-time visibility-scoped supersession & Invariant 4 guards**:
   - `record_supersession` rejects cycles (via recursive CTE), rejects untrusted assertions superseding trusted assertions (Invariant 4), and rejects non-profile assertions superseding `kind = 'profile'`.
   - Retrieval hides an assertion only when a superseding assertion is visible to the caller under RLS, covers the active/requested temporal window, and obeys `a.untrusted OR NOT newer.untrusted`.
   - If a visible `private` supersession hides a candidate, the response's `TaintLabel.private` is set even if the superseding assertion itself is truncated by `limit` or `token_budget`.

4. **Bounded graph expansion**:
   - Hard limits: `MAX_GRAPH_HOPS = 2`, `MAX_GRAPH_EDGES = 24`, `MAX_SEED_ENTITIES = 16`. Requesting `max_graph_hops > MAX_GRAPH_HOPS` returns an error instead of silently clamping.
   - Entity seed lookup matches single tokens and 2-3 word n-grams against indexed `lower(src_name)`/`lower(dst_name)` and `external_id`, and falls back to `(source_id, source_rev)` citations only when no direct entity matched.
