# Server Concurrency: Bounded `PgPool`, `RwLock<State>`, and Per-Person `ETag` Watermarks

## Decision
1. **Zero-dependency bounded Postgres connection pool (`crates/magnus/src/pg_pool.rs`)**:
   - Replaced single-connection `tokio::sync::Mutex<Option<Client>>` bottlenecks in `Recall` (`RECALL_POOL_SIZE = 8`), `Accounts` (`ACCOUNTS_POOL_SIZE = 4`), and `knowledge_index::Index` (`KNOWLEDGE_POOL_SIZE = 4`) with a semaphore-bounded `PgPool`.
2. **Concurrent state reads & zero-clone batch appends (`crates/magnus/src/commit.rs`, `log.rs`, `kernel.rs`)**:
   - Switched `commit::Writer::state` from `Arc<Mutex<State>>` to `Arc<RwLock<State>>` so concurrent HTTP readers never block each other.
   - Added `AtomicI64` `last_seq` on `Writer` and passed `&[(String, Ev)]` into `Log::append_all` to eliminate batch `Ev::clone()`.
3. **Per-person sequence watermarks (`crates/magnus/src/kernel.rs`, `api.rs`)**:
   - Track `(global_seq, HashMap<person_id, seq>)` in `Kernel::emit_all` and drive `api::seq_etag` from `k.seq_for_person(person_id)` so one person's active agent run no longer busts other users' `304 Not Modified` caches.

## Reason
- Eliminates head-of-line blocking across Postgres reads and in-memory `State` reads on multi-user appliances without adding any external crates or altering event-log or RLS invariants.
