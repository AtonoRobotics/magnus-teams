# The checks run on one Node and in one timezone everywhere

**Choice.** `.node-version` names the Node the checks run on (24.22.3). `ci/ensure-node.sh` downloads that official linux-x64 build into the cache and verifies it against the digest pinned in the script. It verifies the cached copy as well as a fresh download, then prints the directory to put first on PATH. `fast.yml`, the release build and `ci/on-strix-beta.sh` all use it. `ci/fast.sh` refuses to start on any other Node, and runs every check with `TZ=Pacific/Kiritimati`. `ci/toolchain-check.sh` runs `ci/fast.sh` with a stand-in `node` and `cargo` to prove both, and checks that each runner puts the pinned Node first.

**Why.** The hosted runner had Node 24.22.3 in UTC, while Strix Beta had its own Node 18.19.1 (past end of life) in PST. On the same commit, `be1ec9e`, `companion-panel-check.mjs` failed on the hosted runner (fast run 44845886917) and passed on Strix (release run 44845886791). The verdict depended on the machine, not on the code. After `decisions/2026-11-20-release-waits-for-fast.md` the hosted run is the gate, so a run on Strix, by hand or in a manual release, has to give the same answer.

**Why that zone.** Pacific/Kiritimati is UTC+14 and has no daylight saving. Its date differs from UTC's for 14 hours of each day. A check that mixes a UTC date (`toISOString`) with a local one therefore fails on most runs, on every machine alike. In UTC that mistake can never show, because the local date and the UTC date are the same. Having no daylight saving means no run lands on an hour that repeats or is skipped.

**Why a script and not `actions/setup-node`.** Strix Beta's runs are not GitHub jobs, so they need the script anyway. One way of getting Node, with a digest committed here, serves both, and adds no third-party action.

**Not covered.** The server's browser sidecar runs on the server's own Node (24.22.2, set in its `magnus.toml`). That is the product's runtime, not the checks', and this change does not touch it.
