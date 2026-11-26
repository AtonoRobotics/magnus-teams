# Shadow recall is detached and read-only; memory erasure follows the tables, not the mode

Date: 2026-11-25
Status: Accepted. This changes parts of `2026-11-25-unified-memory-rollout.md` and `2026-11-25-unified-memory-erasure.md`.

## Decision

1. **Shadow recall is read-only.** `memory::bridge::retrieve_for_person` takes an `AccessTracking`:
   - V2 passes `Record`.
   - Shadow passes `Skip`.
   - Only recall that is served to the model raises `access_count`.
2. **Shadow recall never sits on the live request.** `Kernel::memory_hits` hands the comparison to `memory::bridge::spawn_shadow_comparison` and returns the legacy hits at once.
   - The comparison runs detached and is bounded by `SHADOW_RECALL_TIMEOUT_MS`.
   - It logs the comparison, a failure, or a timeout.
3. **Erasure is gated on the tables existing.** `person_admin::erase_person` calls `memory::erasure::erase_person_if_present`. This replaces `erase_person_if_enabled`.
   - It erases whenever `public.memory_sources` exists, whatever the configured `MemoryMode`.
   - It does nothing on a database that was never migrated for unified memory.

## Reason

- **Shadow must be zero risk for the user.** In Shadow, the v2 query was awaited inline with no timeout, so a slow database slowed every answer for people still on Legacy.
  - It also raised `access_count` on every comparison.
  - That skewed the curator's lesson-cap ranking and stopped recalled rows from being cold-archived, even though the user never saw them.
- **Rolling back to Legacy must not leave a person's data behind.** An operator can move everyone back to Legacy after a Shadow or V2 period, and the memory tables still hold rows.
  - A mode gate would skip those rows and still report erasure complete.
  - Erasure is a promise about the data, so it follows the data, not the configuration.
