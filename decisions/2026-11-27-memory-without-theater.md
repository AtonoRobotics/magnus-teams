# Memory keeps only what runs

The owner, 2026-11-27: "Remove theater from memory."

Removed:

- `prefetch.rs` and `world_map.rs`, a "Step-0 Prefetch" of pointers into
  the prompt. Nothing called them. A logged `prefetch_recorded` event is
  still read, and erasure still clears it.
- The "Clean Model (<= 6 KB, always-on)" in `mental_model.rs` and its
  edit-diff learning. The module's header promised them, but nothing built
  or used either one. What is left is `GET /v1/memory/summary`, kept for
  apps built before this change. The app now reads `/v1/memory`, the same
  facts in one list (newest first), instead of the summary's clean and
  untrusted halves, which it only joined back together.
- `hybrid_score`, a reciprocal-rank fusion score with recency decay and an
  untrusted weight, applied to a single ranked list.
  - With its constant of 60, the difference between the best match and the
    twentieth was smaller than a month of age or the untrusted weight. A
    fact that matched one word, but was newer or clean, came ahead of one
    that matched every word.
  - `memory_search` now serves the order Postgres ranks the matches in:
    best match first, newest first among equal matches.
  - A fact's taint still reaches the run that reads it (`memory_taint`),
    so ranking an untrusted fact higher does not let it act as clean.

Added: a fact the person types into the app is checked with `same_fact`
like `memory_write`. The same fact again returns the memory already kept.

Reason: code that claims to do something it does not do misleads the next
reader, and a ranking built for several sources ranked the one source
wrongly.
