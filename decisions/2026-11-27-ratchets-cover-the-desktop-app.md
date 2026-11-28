# The ratchets cover the desktop app, and the size ratchet works again

**Choice.**

- **Size.** `ci/ratchet.sh size` already listed every `.rs` file in the repository, the desktop app's among them, but since 793f14b (Wave 3) it passed every file. Wave 3 emptied `ci/oversize.txt`, and the check told the baseline from the file sizes with awk's `NR == FNR`, which is true for every line of the second file when the first is empty: each file over the limit became its own baseline. It now tells them apart by name (`FILENAME == ARGV[1]`). No source file is over 800 lines today, so nothing new fails.
- **Clippy.** `ci/ratchet.sh clippy-desktop` holds the desktop app's clippy warnings to `ci/clippy-baseline-desktop`, with the same counting as the main workspace's (one function, two baselines). The desktop app's `Cargo.toml` gets the main workspace's `unwrap_used` and `expect_used` warnings, so the count includes them, as the Code Standard's rule asks. The baseline is today's count, 39: 31 `unwrap`/`expect`, and the rest Windows-only code unused on Linux, complex types and closures. It may only fall.
- It runs in `ci/fast.sh`'s desktop part (decisions/2026-11-27-fast-tier-compiles-the-desktop-app.md), because compiling the desktop app needs Tauri's Linux libraries, which only the desktop job installs.
- `ci/ratchet-check.sh` now checks the size ratchet in a throwaway repository: a desktop or kernel file over 800 lines with an empty baseline fails, as do a new file beside a listed one and a listed file that grew; a test file passes. The old `ratchet.sh` passes both empty-baseline cases. It also checks that the desktop clippy ratchet fails closed and counts against its own baseline. `ci/release-gate-check.sh` fails if `ci/fast.sh` stops running the desktop ratchet.

**Why.** The owner asked for the ratchets to cover the desktop app. The size bug was found doing so: an 857-line `lib.rs` passed.

**Cleared, 2026-11-28.** The owner asked for the 39 to be cleared, and the baseline is 0:
- **Locks.** Every `std::sync::Mutex` lock in `lib.rs`, `store.rs`, `update.rs` and `speech.rs` takes the value of a poisoned lock (`PoisonError::into_inner`), as `store.rs` and `pin.rs` already did. Each lock guards plain state that stays valid when a holder panics.
- **Serializing.** Writing settings or the device certificate returns its serializing error instead of unwrapping it.
- **Startup.** A failed start prints why and exits 1 instead of panicking.
- **Types.** The three complex types are named (`Connection`, `Device`, `Seen`).
- **Windows-only code.** `detect.rs` and `programs.rs` allow the other system's unused code outside the tests, on both systems, not only on Windows.
- **Small fixes.** The two closure warnings and the filter warning are rewritten.

On Strix Beta the desktop part passes with 0 warnings and all 48 tests. A Windows clippy of the test build still finds two unused test helpers in `local.rs`. That build is outside the ratchet.
