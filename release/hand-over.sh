#!/bin/sh
# Leave this run's build where the production server fetches it
# (release/fetch-build.sh, decisions/2026-11-12-release-builds-on-strix.md).
# Run on Strix Beta by the release job's build, from the checkout's root.
#
#   release/hand-over.sh RUN_ID IMAGE_DIR BROKERED_DIR
#
# Writes ~/magnus-teams-release/RUN_ID/ with programs/ (those named in
# release/appliance-programs, from target/release), dist/ (the desktop
# packages), brokered/ (the connectors' server images), image (a link to
# the sandbox image's directory) and image-hash. Keeps the last three runs.
set -eu
run=$1 image=$2 brokered=$3
case $run in *[!0-9]*) echo "RUN_ID: digits only" >&2; exit 2 ;; esac
root=$HOME/magnus-teams-release
out=$root/$run
rm -rf "$out" && mkdir -p "$out/programs" "$out/brokered"
while read -r p; do cp "target/release/$p" "$out/programs/"; done < release/appliance-programs
cp -r dist "$out/dist"
cp "$brokered"/*.cpio "$out/brokered/"
ln -s "$image" "$out/image"
basename "$image" > "$out/image-hash"
ls -dt "$root"/*/ | tail -n +4 | xargs -r rm -rf
