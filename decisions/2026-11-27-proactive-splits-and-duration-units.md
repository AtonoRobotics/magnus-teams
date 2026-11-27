# Proactive Module Splits (`bundle`, `proactive`, `kernel`) & Remaining Duration Unit Suffixes

## Decision
1. **Proactive splits below 700 lines**:
   - Split [`crates/magnus/src/bundle.rs`](../crates/magnus/src/bundle.rs) (782L → 412L) + `bundle/kernel_ops.rs` (378L).
   - Split [`crates/magnus/src/proactive.rs`](../crates/magnus/src/proactive.rs) (781L → 644L) + `proactive/windows.rs` (146L).
   - Split [`crates/magnus/src/kernel.rs`](../crates/magnus/src/kernel.rs) (790L → 700L) + `kernel/types.rs` (99L).
2. **Unit-suffixed `Duration` constants**:
   - Added explicit `_SECS` / `_MS` unit suffixes to 66 private `Duration` constants across 38 files in `crates/magnus/src/`.

## Reason
Keeps every core kernel module well below the 800-line Code Standard ceiling and aligns duration constant naming across the crate.
