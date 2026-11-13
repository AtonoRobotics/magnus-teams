# Strix Beta's build directories are garbage-collected

**Choice.** `ci/on-strix-beta.sh` runs `ci/strix-gc.sh` on Strix Beta before and after every run. The script removes a NAME's checkout and build directory, both under `~/magnus-teams-ci`, in two cases:
- the NAME has been idle for more than a day (`KEEP_IDLE_HOURS`);
- the disk has less than 200 GB free (`MIN_FREE_GB`). Then idle NAMEs go least recently used first, until there is room again.

The current NAME is never removed. A NAME stays while it is running: a marker is set when its run starts and cleared when it ends. A marker older than 12 hours (`MAX_RUN_HOURS`) means the connection died, and the NAME counts as idle. The shared guest image (`sandbox-out`) is never removed. `ci/strix-gc-check.sh` runs in the fast tier and checks these rules against a fake tree.

**Reason.** The owner, 2026-11-13: "strix beta's disk should have garbage collected after builds. It should not be any where near full ever." Every NAME kept its build directory forever, 15–95 GB each. On 2026-11-13 the 1.9 TB disk reached 100% with about 600 GB of stale directories, and the invariant runs failed on Postgres and the guest VMs, not on their own merits.

Removing every directory right after its run would force a full rebuild on each change's next run. Keeping a NAME for a day keeps that change's incremental builds. The free-space floor bounds the disk whatever the run rate.
