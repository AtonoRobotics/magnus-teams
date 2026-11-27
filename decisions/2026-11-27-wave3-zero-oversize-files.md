# Wave 3A & 3B: Zero Oversize Files in the Repository (`ci/oversize.txt` Emptied)

## Decision
1. **Split `crates/magnus/src/events.rs` (1,189L → 748L) and empty `ci/oversize.txt`**:
   - Extracted 47 large, low-reference `Ev` inline struct variants into typed payload structs in `crates/magnus/src/events/payloads.rs` (592L), re-exported via `pub use payloads::*;`.
   - Under `#[serde(tag = "kind", rename_all = "snake_case")]`, newtype struct variants `Ev::Foo(FooPayload)` serialize and deserialize to the exact same flat JSON wire format `{"kind": "foo", ...fields}` as inline struct variants, preserving full event-log and rollback compatibility.
   - Removed the last entry (`crates/magnus/src/events.rs`) from `ci/oversize.txt` — the repository now has **0** oversize source files.
2. **Proactively split the remaining 3 Rust files near the 800-line ceiling**:
   - `crates/magnus/src/config.rs` (799L → 465L) + `crates/magnus/src/config/sections.rs` (341L).
   - `crates/magnus/src/log.rs` (794L → 521L) + `crates/magnus/src/log/migrate.rs` (226L) + `crates/magnus/src/log/content.rs` (63L).
   - `crates/magnus/src/team/mod.rs` (785L → 523L) + `crates/magnus/src/team/change.rs` (272L).
   - (`crates/magnus/src/api/work.rs` was already split in Wave 2 to 682L + `api/schedules.rs` 144L.)

## Reason
- Emptying `ci/oversize.txt` eliminates the last file-size debt in the codebase and prevents future edits from tripping the 800-line size ratchet.
