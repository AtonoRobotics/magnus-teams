#!/usr/bin/env bash
# The sandbox guest for this checkout (rootfs.ext4, and computer.ext4, the
# computer's base image), built from its own sandbox sources
# (the guest program, check_tools.py, the image) and kept by their hash
# under $MAGNUS_SANDBOX_STORE (default ~/magnus-ci/sandbox): built only when
# they change, so tests and the appliance always run the image of the code.
# Links sandbox/out to it and prints its directory. The store keeps this
# image and the one used before it (master's, while a pull request changes
# the image); older ones are deleted, as each is a few gigabytes. Nothing
# outside CI runs from the store: the appliance keeps its own copy
# (release/to-appliance.sh).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here/.."
h=$( { git rev-parse HEAD:sandbox; git rev-parse HEAD:Cargo.lock; } | sha256sum | cut -c1-16)
d="${MAGNUS_SANDBOX_STORE:-$HOME/magnus-ci/sandbox}/$h"
if [ ! -f "$d/rootfs.ext4" ] || [ ! -f "$d/computer.ext4" ]; then
  rm -rf sandbox/out "$d.part" && mkdir -p "$d.part" && ln -s "$d.part" sandbox/out
  sandbox/build.sh >&2
  rm -rf "$d" && mv "$d.part" "$d"
fi
touch "$d"
ls -dt "$(dirname "$d")"/????????????????/ | tail -n +3 | xargs -r rm -rf
rm -rf sandbox/out && ln -s "$d" sandbox/out
echo "$d"
