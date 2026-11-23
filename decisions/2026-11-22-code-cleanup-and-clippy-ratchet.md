# 2026-11-22: Code cleanup, zero production clippy warnings, and ratchet tightening

## Decision
Cleaned up production and test code across the workspace and tightened both CI ratchets:

1. **Zero `unwrap()` / `expect()` outside tests:** Replaced every remaining non-test `.unwrap()` and `.expect()` across `crates/magnus` (`vault.rs`, `sandbox.rs`, `app_tools.rs`, `dev_panels.rs`, `proactive.rs`, `update.rs`), `crates/adapter-anthropic`, `crates/adapter-openai`, `brokered/kit`, and `sandbox/guest` with poison-recovering lock guards, typed `Result` propagation, or clean startup error exits.
2. **Zero clippy warnings outside `tests/invariants/`:** Fixed all remaining clippy lints across production crates and `tests/scratch` (boxing `Response` errors on HTTP helpers and `Body::Said`/`Body::Result` enum variants, packing `boundary::record` and `staging::stage_action` arguments, removing the redundant `Kernel::create_tagged` wrapper, adding type aliases, replacing `sort_by` with `sort_by_key`, using `next_back()`/`rfind()`, removing dead fields/imports/returns, and moving `cli::tests` to the end of `cli.rs`).
3. **Test-only unwrap/expect lint policy:** Configured `[lints.clippy]` in `tests/scratch/Cargo.toml` (and `#![allow(clippy::unwrap_used, clippy::expect_used)]` on `crates/magnus/tests/tools.rs`) to allow `unwrap_used` and `expect_used` in test code, matching `AGENTS.md` and `tests/invariants/src/lib.rs`, while keeping all other clippy lints active. Lowered `ci/clippy-baseline` from `577` to `2` (leaving only the two untouched doc-comment warnings in `tests/invariants/tests/inv4_compartments.rs`).
4. **Named duration constants in `sandbox/guest`:** Replaced bare `Duration::from_millis(20)` and `Duration::from_secs(1)` calls in `sandbox/guest/src/main.rs` with `EXEC_POLL_MS` and `FORK_RETRY_S`.
5. **Oversize test files split:** Split `tests/scratch/tests/app.rs` (814 lines) and `tests/scratch/tests/desktop_app.rs` (1034 lines) into submodules (`app/flows.rs` and `desktop_app/flows.rs`), removing both from `ci/oversize.txt` and tightening `crates/magnus/src/events.rs` to 1235 lines.
6. **Retired team-screen UI cleanup:** Removed dead `noTeamComputer`, `TeamScreens`, `TeamScreenCaptions`, and unused `.team-screen` / `.team-screen-none` CSS rules left behind after retiring the Shared Computer VM.

## Reason
Lowering `ci/clippy-baseline` from 577 to 2 turns clippy into an actual zero-warning gate for every production crate and scratch test instead of hiding real issues behind test-helper unwrap noise. Removing production panics, bare durations, dead UI code, and oversize test files brings the codebase in line with the rules in `AGENTS.md`.
