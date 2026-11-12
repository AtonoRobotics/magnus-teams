#!/bin/sh
# Fetch the release that the Strix build job left on Strix Beta
# (decisions/2026-11-12-release-builds-on-strix.md). The production server
# builds nothing. It fetches the build, signs it, publishes it and installs it.
#
#   release/fetch-build.sh RUN_ID DEST
#
# Copies the build's programs/, dist/ and brokered/ into DEST. Copies its
# sandbox image into the local image store (~/magnus-ci/sandbox/<hash>, or
# MAGNUS_SANDBOX_STORE) only when the store does not already hold it, since
# an image is a few gigabytes and changes only with the sandbox's sources.
# Prints the image's directory.
#
# STRIX_BETA overrides the ssh destination (default the LAN address).
set -eu
[ $# = 2 ] || { echo "usage: release/fetch-build.sh RUN_ID DEST" >&2; exit 2; }
run=$1 dest=$2
case $run in *[!0-9]*) echo "RUN_ID: digits only" >&2; exit 2 ;; esac
host=${STRIX_BETA:-samuel@192.168.0.51}
ssh_="ssh -o BatchMode=yes -o ConnectTimeout=15 -o HostKeyAlias=strix-beta"
src=magnus-teams-release/$run
mkdir -p "$dest"
rsync -a -e "$ssh_" "$host:$src/programs" "$host:$src/dist" "$host:$src/brokered" "$dest/" >&2
h=$($ssh_ "$host" "cat $src/image-hash")
case $h in ????????????????) ;; *) echo "bad image hash from the build: $h" >&2; exit 3 ;; esac
store=${MAGNUS_SANDBOX_STORE:-$HOME/magnus-ci/sandbox}
if [ ! -f "$store/$h/rootfs.ext4" ]; then
  rm -rf "$store/$h.part" && mkdir -p "$store/$h.part"
  rsync -a --sparse -e "$ssh_" "$host:$src/image/vmlinux" "$host:$src/image/rootfs.ext4" "$store/$h.part/" >&2
  mv -T "$store/$h.part" "$store/$h"
fi
touch "$store/$h"
# The store keeps this image and the one before it, as sandbox/image.sh does.
ls -dt "$store"/????????????????/ | tail -n +3 | xargs -r rm -rf
echo "$store/$h"
