# Atomic Person Erasure for Unified Memory

Date: 2026-11-25
Status: Accepted (Phase 4)

## Decision

1. **Single-transaction hard erasure (`crates/magnus/src/memory/erasure.rs`)**:
   - `erase_owner` (single tenant) and `erase_owner_across_tenants` (whole appliance) lock all `memory_sources` owned by the erased person `FOR UPDATE` before reading `memory_evidence`, serializing cleanly against in-flight `FOR SHARE` enrichment and distillation commits.
   - Hard-deletes all owned `memory_sources` (cascading their `memory_evidence` and `memory_enrichment_jobs`), all owned `memory_assertions`, any multi-source distilled `lesson`/`profile` derived from an erased source, any zero-evidence assertions and edges, all remaining `memory_evidence` pointing to deleted targets, and all newly orphaned `memory_entities`.
   - `memory_supersessions` rows cascade automatically via composite foreign keys.

2. **Integration with `person_admin::erase_person` and backups**:
   - `person_admin::erase_person` invokes `memory::erasure::erase_person_if_enabled` right after `log.delete_content`, gated on `cfg.memory.is_unified_enabled()` so `MemoryMode::Legacy` stays zero-risk.
   - Unified memory tables are derived projections (like `public.knowledge`), so backups do not snapshot them directly; restoring a scrubbed backup starts from empty memory tables migrated via `migrate_if_enabled` and only projects non-erased events.
