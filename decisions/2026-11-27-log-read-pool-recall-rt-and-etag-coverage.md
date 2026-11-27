# Log Read Pool, Fewer `Recall::walled` Round-Trips, Full Polled ETag Coverage, and UI Poll Gating

## Decision
1. **ETag `304 Not Modified` on remaining log-derived polled endpoints**:
   - Added `api::global_seq_etag` and wired `304 Not Modified` support on `GET /v1/projects`, `GET /v1/workspaces`, `GET /v1/staged`, and `GET /v1/versions`.
2. **Fewer Postgres round-trips in `Recall::walled` (`crates/magnus/src/recall.rs`)**:
   - Switched to `build_transaction().read_only(true)` and combined `SET LOCAL ROLE` + `set_config('magnus.person', ...)` into one parameterized `SELECT set_config('role', $1, true), set_config('magnus.person', $2, true)` round-trip.
3. **Dedicated read pool on `Log` (`crates/magnus/src/log.rs`)**:
   - Added `readers: PgPool` (`LOG_READ_POOL_SIZE = 4`) on `Log` and routed read-only audit/export/file queries (`output`, `contents`, `content_seqs`, `person_content_seqs`, `of_kinds`, `bundle_history`, `bundle_at`, `removals`, `removed_events`, `run_events`, `dump`) to `readers`, keeping the advisory-locked writer connection dedicated to appends.
4. **UI poll visibility gating & observer consolidation**:
   - Gated `adminPoll` (`admin.js`) and `opsPollTimer` (`admin-ops.js`) on `!document.hidden`, and routed `aurora-stage.js`'s `#main` watch through `AuroraCore.onDom` when present.

## Reason
Eliminates writer-connection contention on heavy log reads, cuts per-recall latency, extends `304` caching to all remaining log-derived polled routes, and stops idle background admin tabs from polling.
