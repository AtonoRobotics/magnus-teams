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
# `STRIX_BETA` overrides the SSH destination. By default it is the wired link
# when it answers, else the LAN address (ci/strix-beta-host.sh); the tailnet
# name has hung. `STRIX_BETA_HOST_KEY_ALIAS` overrides the known-host lookup
# name.
set -eu
[ $# -ge 2 ] || { echo "usage: ci/on-strix-beta.sh NAME COMMAND..." >&2; exit 2; }
name=$1; shift
case $name in *[!A-Za-z0-9._-]*) echo "NAME: letters, digits, dot, dash, underscore" >&2; exit 2 ;; esac
# The wired link when it answers, the Wi-Fi LAN address otherwise
# (ci/strix-beta-host.sh).
read -r host host_key_alias <<EOF
$(sh "$(dirname "$0")/strix-beta-host.sh")
EOF
ssh_="ssh -o BatchMode=yes -o ConnectTimeout=15 -o HostKeyAlias=$host_key_alias"
root=$(cd "$(dirname "$0")/.." && pwd)
dir=magnus-teams-ci/$name
# This NAME is in use from now on, so another run's garbage collection
# (ci/strix-gc.sh) leaves it alone until it ends.
$ssh_ "$host" "mkdir -p magnus-teams-ci/.used magnus-teams-ci/.running && touch magnus-teams-ci/.used/$name magnus-teams-ci/.running/$name"
# Files are compared by content, and a changed file gets the time it was
# copied, not its time here: cargo rebuilds what is newer than its last build,
# and a file kept at an older time would be skipped and leave a stale build.
rsync -rlpD --checksum --delete -e "$ssh_" --exclude .git --exclude target/ --exclude node_modules/ "$root/" "$host:$dir/"
# Some checks read what is committed, such as a script's execute bit
# (release_channels.rs). A worktree's .git is a pointer the copy cannot
# follow, so the copy gets its own repository holding this checkout's HEAD:
# its index is the commit, and the copied files are the working tree.
git -C "$root" bundle create - HEAD 2>/dev/null | $ssh_ "$host" "cat > $dir.bundle && cd $dir && rm -rf .git && git init -q && git fetch -q ../$name.bundle HEAD && git reset -q FETCH_HEAD && rm ../$name.bundle"
# The sandbox's guest kernel and image are built, not checked in. Each run
# uses the image of its own committed sandbox sources, as a release does
# (sandbox/image.sh): built on Strix Beta the first time those sources are
# seen, kept by their hash under ~/magnus-teams-ci/sandbox-images (the newest
# four), and shared by every run with the same sources
# (decisions/2026-11-14-strix-runs-use-their-own-sandbox-image.md). A change
# to sandbox/ is in the image once it is committed. Without an image a task's
# contract checks cannot run, and every scratch test with a task fails, so a
# failed build stops the run.
images=magnus-teams-ci/sandbox-images
image_log=/tmp/magnus-teams-image-$name.log
# The command, quoted for the remote shell.
cmd=
for a in "$@"; do cmd="$cmd '$(printf '%s' "$a" | sed "s/'/'\\\\''/g")'"; done
case "$1 ${2:-}" in "cargo test") cmd="cargo build --workspace --bins &&$cmd" ;; esac
# Node is the pinned one (ci/ensure-node.sh), not Strix Beta's own.
# The browser checks load playwright-core from browser/node_modules, which
# the copy leaves out. Strix Beta's disk is garbage-collected before the run
# and again after it (ci/strix-gc.sh), and the run's exit status is kept.
# A build here keeps line tables only and no incremental state: full debug
# info and incremental caches made each NAME's build directory 80-95 GB,
# which is what filled the disk (decisions/2026-11-13-strix-capacity.md).
gc="STRIX_GC_PG=postgres://postgres:magnus_dev@127.0.0.1:54361/postgres sh \$HOME/$dir/ci/strix-gc.sh $name"
exec $ssh_ "$host" "$gc; (export PATH=\$HOME/.cargo/bin:\$PATH MAGNUS_TEST_PG=postgres://postgres:magnus_dev@127.0.0.1:54361/postgres CARGO_TARGET_DIR=\$HOME/magnus-teams-ci/targets/$name CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_PROFILE_TEST_DEBUG=line-tables-only; cd $dir && node_bin=\$(sh ci/ensure-node.sh) && export PATH=\$node_bin:\$PATH && { [ -d browser/node_modules ] || (cd browser && npm ci --no-audit --no-fund >/dev/null); } && { img=\$(MAGNUS_SANDBOX_STORE=\$HOME/$images MAGNUS_SANDBOX_KEEP=4 sandbox/image.sh 2>$image_log) || { echo 'the sandbox image did not build:' >&2; tail -20 $image_log >&2; exit 1; }; } && echo \"sandbox image: \$(basename \$img)\" >&2 && export MAGNUS_GUEST_KERNEL=\$img/vmlinux MAGNUS_GUEST_ROOTFS=\$img/rootfs.ext4 && $cmd); rc=\$?; touch \$HOME/magnus-teams-ci/.used/$name; rm -f \$HOME/magnus-teams-ci/.running/$name; $gc; exit \$rc"
