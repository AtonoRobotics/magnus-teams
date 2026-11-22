# Generalised Postgres hybrid memory index (superseding DECISIONS.md:503)

**Choice.** Generalise `knowledge.rs`'s existing Postgres `tsvector` + untyped `vector` + `model` column + forced RLS architecture into `crates/magnus/src/memory_index.rs` to index personal memory and bundle entries alongside org knowledge, gated behind a single typed `[memory] mode = "legacy" | "shadow" | "v2"` rollback switch (default `legacy`), with personal memory defaulting to FTS-only (`personal_embeddings = false` per Owner Decision O1).

**Reason.** Supersedes `DECISIONS.md:503` ("a full-text index in Postgres is not needed at a person's memory's size"). Loading all personal memories into RAM for 2/3-prefix substring matching misses synonyms, indirect org references, and temporal updates as history grows, while reusing `knowledge.rs`'s existing Postgres + RLS infrastructure adds zero new operational dependencies and preserves instant 1-line config rollback with zero data loss.
