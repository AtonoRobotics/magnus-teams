# 3-Job Gated Memory Curator

Date: 2026-11-25
Status: Accepted (Phase 3)

## Decision

Implement the background memory curator (`crates/magnus/src/memory/curator.rs`) as three bounded jobs that make **0 LLM calls when idle** (omitting noisy prompt-level EMA feedback weighting):

1. **Watermarked Collision Review** (`review_collisions`):
   - Progress is kept per tenant in `memory_curator_watermarks` (schema version 2) as `(created_at, assertion_id)`, advanced after each candidate. A pass with nothing new reads one row and makes no reviewer calls and no writes. Candidates younger than `COLLISION_WATERMARK_SETTLE_SECS` wait for a later pass, so a write whose transaction began before the watermark moved is never skipped.
   - Candidates and their neighbours are active, embedded, still valid (`valid_to` unset or in the future) and not superseded. Each candidate is checked against at most `COLLISION_NEIGHBOURS_PER_ASSERTION` older neighbours within `COLLISION_MAX_COSINE_DISTANCE = 0.15`, and a pass takes at most `COLLISION_BATCH_LIMIT` candidates.
   - Pair classification is delegated to an injectable `CollisionReviewer` (`MergeDuplicate | TemporalUpdate | Supersede | KeepBoth`):
     - `MergeDuplicate` only within an identical `(kind, owner, readers, untrusted, private)` class. Evidence, access counts **and supersession links** move onto the older assertion before the duplicate is deleted. Without the link move, the cascade would revive whatever the duplicate had superseded. A merge whose moved links would form a cycle is rolled back.
     - `TemporalUpdate` changes the older row for every reader of it. It is refused unless:
       - every reader of the older assertion can read the newer one (`readers_cover`), and the newer one carries no `private` bit the older lacks;
       - the newer one is not untrusted over a trusted older one (Invariant 4);
       - a profile entry is closed only by a profile entry;
       - the newer one starts after the older one.
     - `Supersede` goes through `record_supersession`, which returns `Recorded` or `Refused(reason)`. Refusals are counted as guard skips; database errors fail the pass. No reader-coverage check is needed here, because supersession takes effect at read time only for readers who can see the superseding assertion.
2. **Fenced Lesson & Profile Distillation** (`distill_assertion`):
   - Serializes per `(tenant_id, owner_id)` with a transaction advisory lock, so the profile byte budget and lesson cap are checked against settled totals.
   - Locks every contributing source revision `FOR SHARE`, returning `FencedDeleted` when one was erased.
   - All sources must belong to the distilling owner: the budget and cap are per owner, and one person's material must not fill another's profile.
   - Derives the strictest CNF `readers` intersection and the OR of `(untrusted, private)`, and writes evidence to every input. Erasure of any input therefore deletes the distilled row.
   - Enforces `PROFILE_MAX_BYTES` and `!untrusted` for profiles, and the lesson cap `[memory] max_lessons_per_scope` (default 64, validated at startup). It archives the least-used other lessons, never the lesson just written.
3. **Cold Archival & Orphan Entity Sweep (0 LLM)**:
   - Archives unused (`access_count = 0`) `untrusted` assertions older than `TIER2_COLD_DAYS` (30 days) — never trusted ones — and sweeps orphaned `memory_entities`.
   - Archival is a reversible flag on the row and is not written to the event log. `memory_curator.rs` keeps emitting `Ev::MemoryArchived` for the legacy store. The two curators act on different stores, so a person's memory is curated by whichever store their `MemoryMode` makes authoritative.

## Not yet built

Nothing schedules these jobs yet. A periodic runner, the LLM-backed `CollisionReviewer`, and the `MemoryMode` gate that chooses this curator over the legacy one will arrive together when v2 memory is wired into the kernel. Its interval and daily budget will become `[memory]` settings then, not before. Settings with no reader would be speculative.

## Tests

The live memory tests (Phases 1–3) run on a fresh database and a NOBYPASSRLS kernel role per test, created from `MAGNUS_TEST_PG` (`memory/live_pg_test.rs`). They previously read `MAGNUS_TEST_ADMIN_URL`, which CI never set, so they returned early and passed without running.
