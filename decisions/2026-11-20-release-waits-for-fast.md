# A release waits for its commit's fast run

**Choice.** `release.yml` no longer starts on a push to main. It starts when the `fast` workflow completes for a push to main in this repository (`workflow_run`), and its build job runs only when that run passed. Both its checkouts take the commit the fast run checked (`github.event.workflow_run.head_sha`), not main's newest. A manual run (`workflow_dispatch`) still releases what it is given, after running the checks itself.

The fast tier's checks live in one script, `ci/fast.sh`, which `fast.yml` runs and a manual release runs. It runs every `desktop/tests/*-check.mjs` by name, so a new check cannot be missing from a hand-kept list. `ci/release-gate-check.sh`, run by `ci/fast.sh`, fails if any of this comes undone.

A release whose fast run failed creates a run that builds nothing. It takes a concurrency group of its own, so it cannot cancel a real release waiting its turn.

**Why.** `fast.yml` and `release.yml` both started on the push and ran side by side. The release ran its own, shorter copy of the checks on Strix Beta. Of the 100 main commits from 2026-11-15 to 2026-11-21, 18 reached the production server and the desktop app's production channel while their fast run had failed:

- `sandbox/desktop/test_desktop.py` failed on ten commits between 2026-11-15 and 2026-11-18, and the release never ran it. Fast run 44241046061 failed, and release run 44241045497 shipped the same commit.
- `companion-panel-check.mjs` failed on the hosted runner (Node 24, UTC) and passed on Strix (Node 18, PST). Fast run 44845886917 failed, and release run 44845886791 shipped the same commit.
- Four checks (`draft`, `markdown`, `native-background`, `preferences`) were missing from fast.yml's list and ran only inside the release.

The fast tier is the gate AGENTS.md names, so a release follows its verdict instead of a second, different copy of it.

**Cost.** A release starts once the fast run ends, about six minutes after the push. A release after a fast run no longer repeats the checks on Strix, which took about four minutes of each release build. Push to running on the server grows by about two minutes.

**Not done here.** Pinning one Node version and timezone for both runners, running acceptance before the production channel, and tightening the ratchets are separate changes.

## The lock file is checked as committed

**Choice.** `ci/fast.sh` runs `cargo fetch --locked` straight after `cargo fmt`, before any other cargo command could rewrite `Cargo.lock`. `ci/release-gate-check.sh` keeps it there.

**Why.** The first release after this change (run 44876019783) failed at `cargo fetch --locked`. Main's `Cargo.lock` still listed `magnus-adapter-anthropic` as a dependency of a crate that had dropped it. The release's own copy of the checks had run `cargo test`, which rewrote the lock file in the runner's checkout before the locked fetch read it, and the hosted fast run rewrote its own copy the same way, so nothing ever failed. The gate now reads the lock file as the release build does.
