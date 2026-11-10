# Each Strix Beta run builds into its own directory, from fresh files

`ci/on-strix-beta.sh NAME COMMAND...` now builds into `~/magnus-teams-ci/targets/<NAME>/` instead of one directory every checkout shared. Reason: the scratch and invariant tests start the server and the model adapter as programs from the build directory. Two runs at once with one directory started each other's programs, so a run could pass or fail on another checkout's code.

The copy no longer keeps file times (`rsync -rlpD --checksum`, not `-a`). Files are compared by content, and a changed one gets the time it was copied. Reason: cargo rebuilds what is newer than its last build. A changed file kept at its older time here was skipped, and the tests ran a stale program.

A `cargo test` command runs `cargo build --workspace --bins` first. Reason: `cargo test -p magnus-scratch` does not build the programs its tests start, and without the build they ran whatever was last built. `ci/acceptance.sh` no longer touches each crate's root to force that: the build directory is its own now.

Each NAME costs a full build directory (about 100 GB with the scratch suite). Use one NAME per change and reuse it. Delete `~/magnus-teams-ci/targets/<NAME>` when the change has landed. The old shared `~/magnus-teams-ci/target` is no longer used.

Test: `tests/scratch/src/ci_scripts.rs` runs the script against stand-ins for `ssh` and `rsync` (fast tier).
