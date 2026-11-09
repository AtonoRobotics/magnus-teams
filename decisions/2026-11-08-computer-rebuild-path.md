# The computer's rebuild path

A computer's VM can be thrown away. What makes that true is here.

**Overlay root.** The guest pivots onto an overlayfs with the read-only image below and the overlay drive above. No overlay drive (the per-run sandbox, a helper) means the old read-only root. If the drive is there and does not mount, the guest exits rather than run a computer whose apps have nowhere to go.

**Pinned to the base.** computerd records the base image's id (`images/base.id`, written at install from its sha-256) when it makes an overlay, and drops overlay and snapshot on any other base. A new overlay is reported as `overlay_fresh`, and Magnus replays the manifest onto it.

**Manifest in the log, installers in a cache.** `Change::AppInstalled` holds name, version, source, hash and steps. The installer is read from the computer and hashed by Magnus as it arrives, because the guest's word about it is worth nothing. It stays under `<state_dir>/computer-installers/<sha256>` and is re-hashed before each replay. Step 4's proxy adds the check of source and hash against what it saw.

**Hibernate.** Firecracker's memory and state files are made by the VM's own user inside its jail, so root opens them only with `O_NOFOLLOW`, only as a regular file owned by that uid, and only after the VM is dead. Each is sealed in 1 MiB chunks under a key made for that snapshot and stored root-only beside it. A snapshot is deleted the moment it has been resumed, because the disks move on from it.

**Resets.** computerd itself sends the reset, through the relay, inside `wake`. A caller cannot forget it, and if it fails the VM is killed and the snapshot kept. Network identity is the host name and the MAC of the network card; the address is fixed and private to the computer's own network namespace.

**An edit discards the memory.** A helper VM that can write discards the snapshot when it boots. Reading uses a helper with a read-only drive and a `noload` mount, so looking at a hibernated computer's files does not cost its memory.

**Backups.** Chunks of 4 MiB, named by a keyed hash of their content and sealed under a per-computer key, with a list per backup. All-zero chunks are not stored. A second backup stores only what changed, so this is the chunked copy-on-write of the spec. Nothing uploads yet because there is no object store to upload to. A snapshot is single-use and stays a sealed file.

A running guest freezes its volume during the copy (`FIFREEZE`, thawed by the guest itself after 120 seconds if the host goes away), so the copy is point in time without touching the cgroup a user's pause relies on. No backup holds the lock a kill needs.

**Purge.** Every backup list and snapshot older than `purge_days` (default and maximum 30) is deleted, along with chunks no remaining list uses. `purge` takes the time as an argument so a test can move the clock.

**Deleting a file.** The guest overwrites it with zeros, syncs, then unlinks it, because a freed block would stay readable in the drive image and be copied into every later backup. It does not reach earlier versions of a file left in free blocks by ordinary overwrites. Until those are scrubbed, a canary written, overwritten and only then deleted can stay in the image.

**Telling the agent.** A run's person section lists the files deleted from that person's computers in the 30 days before the run started, fixed by the run's start so a replay reads the same. Workspace computers join this with step 10's entitlements.

**Not here yet.** Browser profiles arrive with step 7, so their backup does too. Idle hibernation needs an idle signal and arrives with the agent's computer tools in step 6.
