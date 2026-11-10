#!/bin/sh
# Run a build or test command for this checkout on Strix Beta, the
# development and CI host (decisions/2026-11-09-builds-on-strix-beta.md), so
# the server's machine does not fill its disk with build output. The checkout
# (without target/ or node_modules/, and with a repository of its own holding
# HEAD) is copied to Strix Beta under
# ~/magnus-teams-ci/<name>/ and the command runs there, with the magnus-teams
# test database (its own Postgres, 127.0.0.1:54361, never magnus-claude's)
# and a build directory of its own, ~/magnus-teams-ci/targets/<name>/.
#
#   ci/on-strix-beta.sh NAME COMMAND...
#   ci/on-strix-beta.sh my-change cargo test --workspace --lib --bins
#   ci/on-strix-beta.sh my-change cargo test -p magnus-scratch --test chat
#
# Each NAME builds into its own directory: the tests start the server and the
# model adapter as programs from the build directory, so two runs sharing one
# would start each other's programs. Use a NAME of your own for each change.
# A `cargo test` command builds every program first (`cargo build --workspace
# --bins`): the scratch and invariant tests run the programs but do not build
# them.
#
# STRIX_BETA overrides the ssh destination (default the LAN address: the
# tailnet name has hung).
set -eu
[ $# -ge 2 ] || { echo "usage: ci/on-strix-beta.sh NAME COMMAND..." >&2; exit 2; }
name=$1; shift
case $name in *[!A-Za-z0-9._-]*) echo "NAME: letters, digits, dot, dash, underscore" >&2; exit 2 ;; esac
host=${STRIX_BETA:-samuel@192.168.0.51}
ssh_="ssh -o BatchMode=yes -o ConnectTimeout=15 -o HostKeyAlias=strix-beta"
root=$(cd "$(dirname "$0")/.." && pwd)
dir=magnus-teams-ci/$name
# Files are compared by content, and a changed file gets the time it was
# copied, not its time here: cargo rebuilds what is newer than its last build,
# and a file kept at an older time would be skipped and leave a stale build.
rsync -rlpD --checksum --delete -e "$ssh_" --exclude .git --exclude target/ --exclude node_modules/ "$root/" "$host:$dir/"
# Some checks read what is committed, such as a script's execute bit
# (release_channels.rs). A worktree's .git is a pointer the copy cannot
# follow, so the copy gets its own repository holding this checkout's HEAD:
# its index is the commit, and the copied files are the working tree.
git -C "$root" bundle create - HEAD 2>/dev/null | $ssh_ "$host" "cat > $dir.bundle && cd $dir && rm -rf .git && git init -q && git fetch -q ../$name.bundle HEAD && git reset -q FETCH_HEAD && rm ../$name.bundle"
# The sandbox's guest kernel and image are built, not checked in, and every
# checkout shares one copy (decisions/2026-11-09-scratch-suites-on-strix-beta.md).
# A checkout that has them refreshes it; without them a task's contract checks
# cannot run, and every scratch test with a task fails.
guest=magnus-teams-ci/sandbox-out
if [ -f "$root/sandbox/out/rootfs.ext4" ] && [ -f "$root/sandbox/out/vmlinux" ]; then
  $ssh_ "$host" "mkdir -p $guest"
  rsync -a --sparse -e "$ssh_" "$root/sandbox/out/vmlinux" "$root/sandbox/out/rootfs.ext4" "$host:$guest/"
fi
# The command, quoted for the remote shell.
cmd=
for a in "$@"; do cmd="$cmd '$(printf '%s' "$a" | sed "s/'/'\\\\''/g")'"; done
case "$1 ${2:-}" in "cargo test") cmd="cargo build --workspace --bins &&$cmd" ;; esac
# The browser checks load playwright-core from browser/node_modules, which
# the copy leaves out.
exec $ssh_ "$host" "export PATH=\$HOME/.cargo/bin:\$PATH MAGNUS_TEST_PG=postgres://postgres:magnus_dev@127.0.0.1:54361/postgres CARGO_TARGET_DIR=\$HOME/magnus-teams-ci/targets/$name MAGNUS_GUEST_KERNEL=\$HOME/$guest/vmlinux MAGNUS_GUEST_ROOTFS=\$HOME/$guest/rootfs.ext4; cd $dir && { [ -d browser/node_modules ] || (cd browser && npm ci --no-audit --no-fund >/dev/null); } && $cmd"
