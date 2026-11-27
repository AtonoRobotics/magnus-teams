# Wave 2: Zero-Copy `Run::frames`, Read Projections, and `seq`-Based Weak `ETag` (`304 Not Modified`)

## Decision
1. **Ref-counted VM screenshots (`crates/magnus/src/state/work.rs`)**:
   - Changed `Run::frames` from `Vec<(u32, String)>` to `Vec<(u32, Arc<str>)>` so any `Run::clone()` bumps a reference count instead of deep-copying megabytes of base64 PNGs (with identical `Debug` formatting for canonical state snapshots).
2. **Zero-copy projections inside `k.read(...)` (`crates/magnus/src/api/work.rs`)**:
   - Projected `timeline()`, `resume()`, `live()`, and `output()` directly inside `k.read(|s| ...)` without cloning `Run`.
3. **`seq`-based weak `ETag` (`304 Not Modified`) on polling endpoints (`crates/magnus/src/api.rs`, `api/work.rs`, `api/schedules.rs`)**:
   - Added `seq_etag`, `not_modified`, and `with_etag` (`W/"seq-{last_seq}"` + `Cache-Control: private, no-cache`) on `GET /v1/tasks`, `GET /v1/inbox`, `GET /v1/chat`, and `GET /v1/schedules`.
   - Split schedule HTTP endpoints into `crates/magnus/src/api/schedules.rs` (keeping `api/work.rs` well below 800 lines).

## Reason
- Polling endpoints hit every 5–10s per tab were re-reading words, projecting full JSON arrays, and serializing unchanged state. Returning `304 Not Modified` on matching `If-None-Match` (already sent by `desktop/ui/bridge.js` from Wave 1A) skips projection and serialization completely when `s.last_seq` has not advanced.
- Eliminating `Run::clone()` on read-only endpoints and ref-counting `Run::frames` avoids multi-MB allocations on busy computer-use runs.
