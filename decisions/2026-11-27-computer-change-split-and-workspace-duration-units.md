# Split `computer/mod.rs` and Add Workspace-Wide Duration Constant Unit Suffixes

## Decision
1. Split [`crates/magnus/src/computer/mod.rs`](../crates/magnus/src/computer/mod.rs) (766L → 554L) by extracting `Change` into `crates/magnus/src/computer/change.rs` (220L) with `pub use change::Change;`.
2. Added explicit `_SECS` / `_MS` unit suffixes to 37 private `Duration` constants across 14 files in `crates/local`, `crates/computerd`, `crates/adapter-proto`, `sandbox/guest`, and `desktop/src-tauri`.

## Reason
Keeps `computer/mod.rs` well under the 800-line ceiling and enforces consistent unit-suffixed duration constants across the entire workspace.
