# Unified Workspace RLS Memory and 2-Role Simplification (Phase 2)

**Choice.** Extend the Wave 2–3 hybrid memory index (`memory_index.rs`) and `MissionLedger` (`team/missions.rs`) with unified workspace memory and 2-role simplification:

1. **Unified `MemoryScope` (`person:<id>` | `workspace:<id>`):**
   - Additive Postgres migration (`WORKSPACE_MEMORY_MIGRATION_SQL`) adds a `scope` column and index to `public.memory_index` so per-person memory and workspace memory share one RLS-enforced hybrid index with zero data loss on rollback.
2. **Automatic Airlock-Only Workspace Indexing:**
   - Only explicitly released `TeamContribution`s (`LaneReleased`) are indexed into `workspace:<id>` memory, deduplicated via `token_overlap_similarity` (`DEDUP_TOKEN_OVERLAP_THRESHOLD`). Unreleased `private_draft`s never enter workspace memory.
   - All workspace memory reads carry `untrusted: true` so teammates cannot inject another member's Personal Agent.
   - Exposed via `GET /v1/workspaces/{ws}/memory?q=...` (`MissionLedger::search_workspace_memory`).
3. **2-Role Simplification (`TeamRole::Member | TeamRole::Admin`):**
   - Maps legacy 4-tier `Role` (`Guest | Member -> Member`, `Admin | Owner -> Admin`) while preserving `Role` in event replay.

**Reason.** Replaces manual Team KB curation bureaucracy with automatic, Airlock-scoped workspace memory that integrates directly with Magnus's hybrid memory and security model.
