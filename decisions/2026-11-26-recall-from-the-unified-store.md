# Everyone recalls from the unified store, which every memory write updates directly

Date: 2026-11-26
Status: Superseded the same day by `2026-11-26-recall-searches-memory-where-it-is-kept.md`. It replaced:
- the modes in `2026-11-25-unified-memory-rollout.md` (Legacy, Shadow, V2, `person_modes`);
- `2026-11-25-memory-background-sync.md` and its amendment;
- part 2 of `2026-11-25-shadow-read-only-and-erasure-by-presence.md` (the Shadow comparison).

## Decision

- **Recall.** `memory_search` is served from the unified store for everyone. The old recall, the modes, the Shadow comparison and the fallback to legacy are removed. `[memory] mode` and `person_modes` in an older config load and are ignored.
- **Searching.**
  - A long word matches by its stem, as a prefix: "allergy" finds "allergic".
  - A query with no words lists the person's memories newest first.
- **What is served.** The store ranks the memories. What is served is each cited memory as the record holds it, current in the state (`bridge::cited_current`), so provenance, taint and read copies stay the memory's own.
- **Writes.** `Kernel::emit_all` updates the store as part of the write (`Kernel::mirror_memory`):
  - a save or a restore copies the memory in;
  - a forget, trash, archive or supersession deletes its copy;
  - a failure is returned as the write's error.
- **Boot.** `Kernel::mirror_all_memory` brings the store level with the state once, before serving. A failure fails the boot.
- **Migration.** `magnus migrate` always creates the memory tables. pgvector is in the production image and in `config/install-postgres.sh`.

## Reason

The store is in the same Postgres as the event log. When it is down, Magnus is down, so a fallback to legacy recall, a background catch-up, waits, timeouts and per-person marks protected against nothing. They added states in which recall was partial or quietly different.

A direct write keeps the store exact at every write. Checking each hit against the state means a stale copy, from a failed write or a crash before the boot catch-up, is never served.
