#!/usr/bin/env bash
# The sandbox guest for this checkout (rootfs.ext4, and computer.ext4, the
# computer's base image), built from its own sandbox sources
# (the guest program, check_tools.py, the image, and crates/computerd-proto,
# whose requests the guest reads) and kept by their hash
# under $MAGNUS_SANDBOX_STORE (default ~/magnus-ci/sandbox): built only when
# they change, so tests and the appliance always run the image of the code.
# Links sandbox/out to it and prints its directory. The store keeps this
# image and the ones used just before it, $MAGNUS_SANDBOX_KEEP in all
# (default 2: master's, while a pull request changes the image; Strix Beta's
# test runs keep more, as several branches run there at once); older ones are
# deleted, as each is a few gigabytes. One build at a time per store: two
# runs that need the same new image wait for one build. Nothing outside CI
# runs from the store: the appliance keeps its own copy
# (release/to-appliance.sh).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here/.."
keep="${MAGNUS_SANDBOX_KEEP:-2}"
case $keep in '' | *[!0-9]* | 0) echo "MAGNUS_SANDBOX_KEEP: a whole number above 0" >&2; exit 2 ;; esac
h=$( { git rev-parse HEAD:sandbox; git rev-parse HEAD:crates/computerd-proto; git rev-parse HEAD:Cargo.lock; } | sha256sum | cut -c1-16)
store="${MAGNUS_SANDBOX_STORE:-$HOME/magnus-ci/sandbox}"
d="$store/$h"
mkdir -p "$store"
exec 9> "$store/.lock"
flock 9
if [ ! -f "$d/rootfs.ext4" ] || [ ! -f "$d/computer.ext4" ]; then
  rm -rf sandbox/out "$d.part" && mkdir -p "$d.part" && ln -s "$d.part" sandbox/out
  sandbox/build.sh >&2
  rm -rf "$d" && mv "$d.part" "$d"
fi
touch "$d"
ls -dt "$store"/????????????????/ | tail -n +$((keep + 1)) | xargs -r rm -rf
rm -rf sandbox/out && ln -s "$d" sandbox/out
echo "$d"
