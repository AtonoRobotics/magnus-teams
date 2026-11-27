# The fast tier compiles and tests the desktop app

**Choice.** Every push compiles the desktop app (`desktop/src-tauri`), runs its tests and checks its formatting and lock file, in a `desktop` job of `fast.yml` that runs beside the other checks.

- `ci/fast.sh` has two parts: `checks` (everything it ran before) and `desktop`. With no argument it runs both, so a manual release and a run on Strix Beta check the desktop app too. `fast.yml` runs each part in a job of its own, so the desktop app adds no time to the run.
- The desktop part is `cargo fetch --locked`, `cargo fmt --check` and `cargo test --locked --lib --bins`, each with `--manifest-path desktop/src-tauri/Cargo.toml`.
- The hosted runner lacks Tauri's Linux libraries; `ci/desktop-libs.sh` installs them (WebKitGTK 4.1, libxdo, D-Bus). The job caches `desktop/src-tauri/target` on its own.
- `in-order` needs both jobs, so the run's verdict, which `release.yml` waits for, fails when either fails.
- `ci/release-gate-check.sh` fails if `fast.yml` stops running either part, if `in-order` stops waiting for both, or if `ci/fast.sh` stops testing the desktop app.

**Why.** The desktop app is a Cargo workspace of its own, so `cargo test --workspace` never compiled it. On 2026-11-27, b7cdb25 imported `futures::StreamExt` in `desktop/src-tauri/src/events_sse.rs`, a crate the app does not depend on. Every fast run passed, and every release after it failed at `cargo tauri build`, unseen until 23d7660 fixed the import. A release that fails after its fast run passed leaves the server on an older version with no verdict on the push that broke it.

**Landing it.** Nothing had checked the desktop app's formatting either, so it is formatted in the same commit (whitespace only). Formatting put `desktop/src-tauri/src/lib.rs` at 852 lines, over the 800-line limit, so its tests moved to `lib_tests.rs`, as `bridge_tests.rs` and `detect_tests.rs` already are; it is 757. On Strix Beta the desktop part passes in 37 s with a warm cache (48 tests), and with b7cdb25's import put back it fails with `unresolved import futures`.

**Limits.** The ratchets came to cover the desktop app later the same day (decisions/2026-11-27-ratchets-cover-the-desktop-app.md).
