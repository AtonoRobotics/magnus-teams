# Recall searches each person's memory where it is kept; nothing is copied

Date: 2026-11-26
Status: Accepted, at the owner's direction ("prove it's the best way or change it"). This replaces `2026-11-26-recall-from-the-unified-store.md`.

## Decision

- **No copy.** `memory_search` is one ranked full-text query over the person's current memories, run in the database where their content is kept (`Recall::search`). It runs inside the person's content wall, as every recall does.
  - Saves, forgets, trash, restore, archive, supersession and erasure need no extra step, because nothing else holds the text.
- **Current memory** is the state's: in the library, not archived, not superseded (`State::current_memory`). The query is limited to those events' content rows.
- **Matching.** Words of 3 or more letters are searched, each as a prefix. A long word matches by its stem: "allergy" finds "allergic".
  - The fact and the topic are both searched.
  - The database lowercases the query as it does the text.
  - Words are runs of letters and digits, so no query syntax gets through.
- **Ranking.** Matches are ranked by text rank in Postgres, then scored with recency and trust (`hybrid_score`), as before. At most `MAX_MEMORY_HITS` (20) are returned.
- **No words.** A query with no words to search for lists the newest memories.
- **Removed.** The mirror on save, the boot catch-up and `memory::bridge` are gone.
  - Recall no longer reads the `memory_sources` engine, and no memory write queues enrichment jobs.

## Reason

A second copy of each memory has to be kept in step through every kind of change, a crash between writes, backup restore and first boot. Every defect found in review came from that:
- a save that committed and then reported failure, so a retry duplicated it;
- an erasure interrupted by a crash that left copies behind;
- stale copies crowding out real ones;
- a growing queue of enrichment jobs that nothing claims;
- a slow first boot.

The copy bought nothing in production. The enrichment worker, embeddings and graph that would justify it do not run, so it was a text-search index over the same words. Searching the record directly gives the same search with nothing to keep in step. The memory engine was this branch's own design (`2026-11-25-unified-postgres-memory.md`); the product spec does not ask for it.
