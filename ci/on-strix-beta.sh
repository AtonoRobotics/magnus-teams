#!/bin/sh
# Run a build or test command for this checkout on Strix Beta, the
# development and CI host (decisions/2026-11-09-builds-on-strix-beta.md), so
# the server's machine does not fill its disk with build output. The checkout
# (without .git, target/ or node_modules/) is copied to Strix Beta under
# ~/magnus-teams-ci/<name>/ and the command runs there, with the magnus-teams
# test database (its own Postgres, 127.0.0.1:54361, never magnus-claude's)
# and one shared build directory.
#
#   ci/on-strix-beta.sh NAME COMMAND...
#   ci/on-strix-beta.sh my-change cargo test --workspace --lib --bins
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
rsync -a --delete -e "$ssh_" --exclude .git --exclude target/ --exclude node_modules/ "$root/" "$host:$dir/"
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
# The browser checks load playwright-core from browser/node_modules, which
# the copy leaves out.
exec $ssh_ "$host" "export PATH=\$HOME/.cargo/bin:\$PATH MAGNUS_TEST_PG=postgres://postgres:magnus_dev@127.0.0.1:54361/postgres CARGO_TARGET_DIR=\$HOME/magnus-teams-ci/target MAGNUS_GUEST_KERNEL=\$HOME/$guest/vmlinux MAGNUS_GUEST_ROOTFS=\$HOME/$guest/rootfs.ext4; cd $dir && { [ -d browser/node_modules ] || (cd browser && npm ci --no-audit --no-fund >/dev/null); } && $cmd"
