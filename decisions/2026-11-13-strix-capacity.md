# Strix capacity: lean builds, a size cap, and no leftover test databases

The owner, 2026-11-13: "Strix alpha and beta have 2tb drives. There is no
reason they should ever be out of space."

What was found on Strix Beta (1.9 TB): 205 GB free, kept there only by
`ci/strix-gc.sh` evicting whole build directories. `~/magnus-teams-ci` held
757 GB, almost all of it build directories of 76-95 GB each; one was 85 GB of
`deps` (2,554 files: every old build of every test binary, which cargo never
deletes) and 9 GB of incremental state. The test Postgres held 130 leftover
databases and allowed 100 connections, and the lib tests failed with "too
many clients", which looked like a capacity failure. Strix Alpha had 675 GB
free and needed nothing.

Decided:
- `ci/on-strix-beta.sh` builds with `CARGO_INCREMENTAL=0` and line tables
  only (`CARGO_PROFILE_DEV_DEBUG`/`CARGO_PROFILE_TEST_DEBUG=line-tables-only`).
  Backtraces keep file and line; nothing here uses a debugger. A run is copied
  fresh each time, so incremental state bought little.
- `ci/strix-gc.sh` empties the current NAME's build directory when it passes
  `MAX_TARGET_GB` (40), so stale builds cannot accumulate inside one NAME.
- With `STRIX_GC_PG` set, the GC drops every test database but `postgres`
  when no NAME is running (the tests create one per server and account and do
  not always drop it). It never drops while another run is in progress.
- The test Postgres allows 500 connections (`ALTER SYSTEM`, on the container
  `magnus-teams-test-pg`).
- `ci/strix-gc-check.sh` covers the size cap and the database sweep.

Not touched, as they are not this project's: `/var/lib/containerd` (230 GB),
`~/actions-runner` (218 GB), `~/.cache` (146 GB) and `/var/lib/docker` (55 GB)
on Strix Beta.

Result, measured the same day: a full build of the workspace's libraries and
programs is 2.5 GB with these flags (it was 80-95 GB). With the old build
directories removed (nothing was running), Strix Beta has 950 GB free, 47%
used, and the test Postgres holds only `postgres`.
