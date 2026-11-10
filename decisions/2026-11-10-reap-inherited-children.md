# Collect the children an update leaves behind

An update or restart in place execs the new release in the same process. The
adapters, microVMs and sidecars the old image started die by PR_SET_PDEATHSIG,
but stay zombies of the process: the new image has no handle to wait on them.
The teams server had 69 `magnus-adapter-` zombies after a day of updates.

`serve` now lists its children from /proc before it spawns anything and waits
for each on a reaper thread (`crates/magnus/src/reap.rs`). It waits only on
those ids, never `waitpid(-1)`, so it cannot take an exit status a live handle
owns. Children started by the running image were already waited on: adapters,
VMs and the browser sidecar kill and wait on drop.
