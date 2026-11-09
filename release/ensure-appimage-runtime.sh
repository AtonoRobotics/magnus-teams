#!/bin/sh
# appimagetool otherwise downloads the moving continuous runtime during
# packaging. Its HTTP 504 stopped master #516 before tests or publication.
# Pin the official asset and verify both fresh downloads and cached bytes.
set -eu
cache=${1:-${XDG_CACHE_HOME:-$HOME/.cache}/magnus-release}
runtime="$cache/appimage-runtime-x86_64-fedfad51"
digest=617aa77314d57c8b8e4fae9c3f0b35af7b30d5529cb69e98f64f92510ed98a46
valid() { printf '%s  %s\n' "$digest" "$1" | sha256sum --check --status; }
mkdir -p "$cache"
if ! valid "$runtime" 2>/dev/null; then
  download=$(mktemp "$runtime.XXXXXX")
  trap 'rm -f "$download"' EXIT HUP INT TERM
  curl --fail --location --retry 3 --retry-all-errors --retry-delay 2 \
    --connect-timeout 15 --max-time 120 \
    --header 'Accept: application/octet-stream' \
    https://api.github.com/repos/AppImage/type2-runtime/releases/assets/657374641 \
    --output "$download"
  valid "$download" || { echo 'AppImage runtime checksum mismatch' >&2; exit 1; }
  mv "$download" "$runtime"
fi
printf '%s\n' "$runtime"
