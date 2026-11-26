# MemoryMode Shadow & V2 Kernel Rollout

Date: 2026-11-25
Status: Accepted (Phase 5)

## Decision

Wire `MemoryConfig::mode_for(person)` (`Legacy | Shadow | V2`) into `Kernel::memory_hits` (`crates/magnus/src/boundary/call.rs`) through `crates/magnus/src/memory/bridge.rs`:

- **`MemoryMode::Legacy`** (default): executes the existing `remembered` path untouched.
- **`MemoryMode::Shadow`**: executes both legacy and `memory::bridge::retrieve_for_person`, logs a structured comparison (`legacy_count`, `v2_count`, `v2_elapsed_ms`, error if any), and returns legacy results so live traffic can be benchmarked with zero user risk.
- **`MemoryMode::V2`**: serves `memory::bridge::retrieve_for_person` directly (calling `MemoryStore::record_access` for returned assertions and propagating `Response.label.private` onto returned `Remembered` entries so `memory_taint` never drops private supersession influence), falling back to legacy on empty query or transient DB error.
