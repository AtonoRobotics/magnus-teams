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

## Amended 2026-11-26 after review

These changes follow from the commitments above:

- **Trash is followed.** `Trashed` marks the person. `Restored` marks the memory's owner, as the state now has it. `Removed` needs nothing, because the earlier `Trashed` already took the memory out of current memory.
- **"Pending" includes the pass that is running.** A pass's marks stay in flight until it has finished. Until then, everyone it carries forgets for, and every person it carries, stays out of step. Before this, a forget became "done" when the pass started rather than when its delete committed.
- **One person's failure keeps only that person marked.** The rest are synced, and the reason is kept (`MemorySync::failure`).
- **Erasure and copying cannot interleave.** A person's sync holds their erasure lock shared from reading their content to copying it.
- **V2 serves only hits that cite current memories.** `bridge::only_current` drops any hit citing a memory that is no longer current, checked against the state when the answer is served. A change landing between the in-step check and the search is never served.
- **Missing tables keep their marks only when needed.** That applies when some mode needs the tables, so a later migration is picked up. With Legacy everywhere, nothing is kept.
