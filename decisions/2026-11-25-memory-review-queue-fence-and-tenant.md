# Collision review is a queue flag, erasure fences ingest, the tenant is configured, and V2 falls back when it finds nothing

Date: 2026-11-25
Status: Accepted. This changes parts of `2026-11-25-unified-memory-curator.md`, `2026-11-25-unified-memory-erasure.md` and `2026-11-25-unified-memory-rollout.md`.

## Decision

1. **Collision review queue.** `memory_assertions.collision_pending` (schema version 3) replaces the time-based `memory_curator_watermarks`. Version 3 drops that table.
   - `curator::review_collisions` reviews pending rows that are committed, embedded, active and unsuperseded.
   - Each pending row is compared with neighbours that have already been reviewed. The pair is oriented by `(created_at, assertion_id)`, and the flag is then cleared.
   - A row that is ineligible when a pass runs stays pending until it qualifies.
   - `COLLISION_WATERMARK_SETTLE_SECS` is removed.
2. **Erasure fences ingest.** The fence is a per-owner advisory lock, `(OWNER_ERASURE_LOCK_NAMESPACE, hashtext(owner_id))`.
   - `ingest_source` takes it shared.
   - `erase_owner_scoped` takes it exclusively before locking the owner's sources `FOR UPDATE`.
3. **The tenant is configuration.** `[memory] tenant_id` defaults to `"default"` and is validated at startup.
   - Unified recall, access recording, write-through and backfill all use it.
   - Nothing in the logic names a tenant.
4. **V2 falls back when it has no hits.** `bridge::v2_hits_or_fallback` serves the v2 hits only when there are some. No hits, or an error, means legacy answers.

## Reason

1. **The watermark could skip rows for good.**
   - `created_at` is when the writing transaction started, so a transaction held open past the settle window committed behind the watermark and was never reviewed.
   - A row that was superseded or unembedded at scan time was passed over permanently, even once it qualified.
   - A flag cleared only after review cannot skip a committed row. Comparing against reviewed rows means every pair is seen once, when the second of the two is reviewed.
2. **Locking existing rows did not stop a concurrent ingest.** `FOR UPDATE` locks only sources that already exist.
   - An ingest committed during erasure survived it, or lost its source without its derived rows being collected.
   - The shared/exclusive lock makes erasure wait for in-flight ingests and hold off new ones until it commits.
3. **A tenant in code is a hidden promise about whose data is recalled.** The Code Standard requires config to be typed, validated and passed in.
4. **The unified store only holds what has been ingested into it.** Serving its empty answer would hide facts the person has in legacy memory.
