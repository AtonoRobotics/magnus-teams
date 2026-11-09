# The person's own computer

**Two checks, on two sides.** `magnus-local` runs on the person's machine, connects out with their token, and takes jobs over a long poll. The server refuses a path that is plainly outside the granted folders. The machine resolves every path itself and refuses anything that leaves them, including through a symlink, because only the machine can see those. Jobs run one at a time, so one job cannot swap a link under another's check.

**No connection, nothing runs.** With no computer connected, or none polling, a local tool is denied before anyone is asked. A computer counts as online while a poll is open or for 5 seconds after one.

**Modes.** A computer is set to ask, allow or never, and the admin's `[local] ceiling` takes the lower of the two, so a ceiling only tightens. Ask means every action, reads included, and an approval covers that one action.

**What needs what.** A read in a granted folder passes. A new file goes through Auto Review, and the write cannot overwrite. An edit, a delete or a command asks unless a rule allows it, and Auto Review cannot clear it alone. Reading taints the run as untrusted and private. A local write does not count as sending data out, because it stays on the machine.

**Commands are confined.** `local_exec` runs under bubblewrap with `/usr` read-only, the granted folders, no home directory and no network, so it cannot reach a keychain or send anything out. Where it cannot be confined, on macOS and Windows for now, it refuses. An install that needs the network does not work yet.

**Not here yet.** The desktop app does not run this: it is a separate binary and the Tauri app has no screen for it. Pausing while the person uses the mouse or keyboard needs the app. There is no screen, camera or microphone access.
