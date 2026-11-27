# One memory store, and everyone starts empty

Date: 2026-11-27
Status: Accepted, at the owner's direction. It follows
`2026-11-26-recall-searches-memory-where-it-is-kept.md` and
`2026-11-27-memory-is-what-the-person-said.md`.

## Decision

Memory stays where it is: `Ev::Memory` events, with the fact and topic in
their content rows. Recall is one ranked full-text query over a person's
current memory, in place.

**Everyone starts empty.** The owner: "Start everyone empty." and "The old
memory should not be used as of the new memory update."
- At boot, after replay and before serving, the kernel archives every
  memory written before `MEMORY_FRESH_START` that is not already archived
  or superseded. It writes one `Ev::MemoryArchived` for each, with reason
  `"fresh_start"`, in one batch. `MEMORY_FRESH_START` is
  2026-11-27T18:47:00Z, when the person-only write rules (3d0e543,
  test.392) went live on the server.
- Memories in the trash are archived too, so putting one back does not
  serve it again.
- Once done, a later boot finds nothing to archive and writes nothing.
- Archived memories stay in the log as history. Erasing a person still
  deletes their content, archived or not.

**No reader serves an archived memory.**
- `Kernel::remembered` reads only current memory. That covers
  `GET /v1/memory`, `/v1/memory/summary`, `memory_search`, the memory pass's
  "Already remembered" list, the `memory_write` duplicate check and the
  person data an admin reads.
- The summary no longer has `archived_facts`.
- Forget, trash and restore treat an archived memory as not found. The trash
  list leaves it out. It is removed with the rest when its time comes.
- The admin overview counts only current memories.
- The dev panel's "Memory writes by label" still counts every write: it
  reports what runs wrote and serves no fact.

**The memory engine is deleted.** `crates/magnus/src/memory/` (store,
schema, ingest, enrich, graph, retrieve, curator, erasure) and
`memory_curator.rs` are gone, with what in `memory_index.rs` only they used.
`memory_index.rs` keeps the write caps, `fact_refusal`, `same_fact` and the
score recall sorts by.
- `magnus migrate` and `magnus restore` no longer create the `memory_*`
  tables. Erasure no longer looks in them.
- The `memory_*` tables already in production are left as they are. They
  are empty, and nothing drops them.
- pgvector and the install scripts are unchanged.
- A config with a `[memory]` section still loads. Its keys are ignored, with
  a warning, as `[admin]`'s are.
- `MemorySuperseded` and `MemoryArchived` keep their readers. Nothing writes
  `MemorySuperseded` now.

## Reason

Production held 613 memories filed under the old rules: status reports, run
checkpoints and duplicates. The owner: "Neither of those should ever be
memories. They are system dialog." Deleting them would lose history the log
keeps for everything else. Archiving keeps the history and serves none of it.

The engine was this branch's own design, the spec does not ask for it, and
recall stopped reading it on 2026-11-26. Code nothing uses still has to be
built, read and kept safe. One store means one place to search, forget and
erase.
