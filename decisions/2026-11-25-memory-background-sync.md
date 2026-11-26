# The unified memory store follows people's memory through one background sync

Date: 2026-11-25
Status: Accepted. This replaces the live write-through and first-search backfill from `3dee411`, recorded under "Live Write-Through & Automatic Backfill" in `2026-11-25-unified-memory-rollout.md`.

## Decision

`memory::sync` keeps each person's sources in the unified store equal to their current legacy memory. Current means not archived and not superseded.

- **Marking.** `Kernel::emit_all` only marks what a write touched (`MemorySync::follow`). It does no database work:
  - `Memory`, `MemorySuperseded`, `MemoryArchived` and `PersonErased` mark the person;
  - `MemoryForgotten` marks the memory id.
- **The pass.** A background task runs a pass when woken, and every `SYNC_RETRY_EVERY_SECS` while marks remain. Everyone is marked at start.
  - Each pass is bounded by `SYNC_PASS_TIMEOUT_MS`.
  - A pass that fails or times out keeps its marks.
- **What a pass does for each person.**
  - It deletes sources that are no longer current, in every mode, whenever the tables exist.
  - It copies missing current memories only for people not on `MemoryMode::Legacy`.
  - It deletes every source carrying a forgotten id.
- **When V2 and Shadow use the store.** Only for a person `MemorySync::is_synced` reports in step: a pass finished for them, nothing marked them since, and no forget is pending. Otherwise V2 answers from legacy and Shadow skips its comparison.

## Reason

The write-through had five problems:

1. It ignored forgets while everyone was on Legacy, and dropped forgets that failed, so a forgotten memory could come back under V2.
2. It ignored archives and supersessions.
3. A save before the first search blocked the backfill for good.
4. A failed copy was never retried.
5. Saves and V2 answers waited on the unified store.

One comparison against the state fixes all five, because the state already folds in forgets, archives, supersessions and erasure. It doesn't matter what order events arrive in, or which ones a failed pass missed. Serving only people who are in step means V2 never shows partial history or a memory waiting to be deleted. While a person is behind, legacy recall, the existing behaviour, answers.
