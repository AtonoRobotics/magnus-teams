#!/bin/sh
# The Node the checks run on, the same on every machine
# (decisions/2026-11-20-one-node-and-one-timezone.md): the version in
# .node-version, the official linux-x64 build, checked against the digest
# pinned here, so a download and a cached copy are both verified. Prints the
# directory holding `node` and `npm`, for PATH:
#
#   PATH="$(sh ci/ensure-node.sh):$PATH"
#
# fast.yml, the release build and ci/on-strix-beta.sh all use it.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
version=$(tr -d ' \n' < "$root/.node-version")
# One line per pinned version, from https://nodejs.org/dist/v<version>/SHASUMS256.txt.
case $version in
  24.22.3) digest=4f2b175a6d53a5c617a80dba2676238a0688cef28c6da4eba35c7d844e66fcfb ;;
  *) echo "ci/ensure-node.sh has no digest for Node $version (.node-version)" >&2; exit 1 ;;
esac
cache=${MAGNUS_NODE_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/magnus-node}
name=node-v$version-linux-x64
dir=$cache/$name
archive=$cache/$name.tar.xz
valid() { printf '%s  %s\n' "$digest" "$1" | sha256sum --check --status; }
mkdir -p "$cache"
if ! valid "$archive" 2>/dev/null; then
  download=$(mktemp "$archive.XXXXXX")
  trap 'rm -f "$download"' EXIT HUP INT TERM
  curl --fail --location --silent --show-error --retry 3 --retry-all-errors --retry-delay 2 \
    --connect-timeout 15 --max-time 300 \
    "https://nodejs.org/dist/v$version/$name.tar.xz" --output "$download"
  valid "$download" || { echo "Node $version checksum mismatch" >&2; exit 1; }
  mv "$download" "$archive"
  rm -rf "$dir"
fi
# Unpacked from the verified archive; unpacked again if it is not whole.
if [ "$("$dir/bin/node" --version 2>/dev/null)" != "v$version" ]; then
  rm -rf "$dir" "$dir.tmp"
  mkdir -p "$dir.tmp"
  tar -xJf "$archive" -C "$dir.tmp" --strip-components=1
  mv "$dir.tmp" "$dir"
fi
printf '%s\n' "$dir/bin"
