#!/bin/sh
# Which key signs this run, and whether the run may publish.
#
#   release/signing-key.sh DEST
#
# Prints the key path, then 1 or 0. 1 is the build box's release key
# (~/.magnus-release/update.key, or MAGNUS_RELEASE_KEY): this run may
# publish and install. 0 is a throwaway key written under DEST: the
# installers can be built and tested, and nothing is published. A runner
# without the file used to hand tauri the path, which it decoded as the
# key and rejected at the first "." (run 41948685783, build-box-beta).
set -eu
dest=$1
real=${MAGNUS_RELEASE_KEY:-$HOME/.magnus-release/update.key}
if [ -f "$real" ]; then
  printf '%s\n1\n' "$real"
  exit 0
fi
mkdir -p "$dest"
cargo tauri signer generate --ci -p "" -f -w "$dest/key" >/dev/null
printf '%s\n0\n' "$dest/key"
