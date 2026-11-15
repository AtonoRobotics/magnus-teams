# Strix runs use their own sandbox image; releases ship it as before

**What happened.** The attachment work added ffmpeg and p7zip to `sandbox/Dockerfile`. A release already ships the new image. `sandbox/image.sh` keys the image by the git hash of `sandbox/`, `crates/computerd-proto` and `Cargo.lock`, so the first `release.yml` run after the merge builds it on Strix. `release/to-appliance.sh` then installs it with the programs of the same release, and a server never runs the new rebuild with the old image. Tests were the gap. `ci/on-strix-beta.sh` ran every test against one shared copy (`~/magnus-teams-ci/sandbox-out`), refreshed only when a checkout happened to have `sandbox/out`. It still held the image of 2026-11-09, so a test of a sandbox change ran against an older image, or passed only against a copy built by hand.

**Choice.** Each Strix run uses the image of its own committed sandbox sources, as a release does. `ci/on-strix-beta.sh` runs `sandbox/image.sh` on Strix with the store `~/magnus-teams-ci/sandbox-images`. The image is built the first time its sources are seen and then shared by every run with the same hash. A failed build stops the run and shows the end of its log.

- `sandbox/image.sh` takes a lock on its store, so two runs that need the same new image wait for one build instead of both writing one directory. `MAGNUS_SANDBOX_KEEP` sets how many images the store keeps. A release keeps 2, as before. Strix keeps 4, because several branches run there at once.
- `ci/strix-gc.sh` leaves `sandbox-images` alone, as it leaves `sandbox-out`, and `ci/strix-gc-check.sh` checks that.
- `sandbox/build.sh` copies the guest from `CARGO_TARGET_DIR` when one is set, as `brokered/build.sh` does. It failed under `ci/on-strix-beta.sh`, which sets one per NAME.
- A change to `sandbox/` reaches a run's image once it is committed. The hash is of the commit, as in a release.

The old shared copy is no longer read. It can be deleted from Strix once no branch from before this change still runs there.
