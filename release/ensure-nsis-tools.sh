#!/bin/sh
# Tauri 2.10.4 checks NSIS.exe even on Linux and otherwise recreates its
# cache, downloading the helper DLL without retries. Use the real upstream
# executable and DLL with exactly the hashes its bundler expects (#516).
set -eu
cache=${1:-${XDG_CACHE_HOME:-$HOME/.cache}/tauri}
mkdir -p "$cache/NSIS/Plugins/x86-unicode/additional"
stage=$(mktemp -d "$cache/.nsis.XXXXXX")
trap 'rm -rf "$stage"' EXIT HUP INT TERM
valid() { printf '%s  %s\n' "$1" "$2" | sha256sum --check --status; }
fetch() {
  curl --fail --location --retry 3 --retry-all-errors --retry-delay 2 \
    --connect-timeout 15 --max-time 120 \
    --header 'Accept: application/octet-stream' "$1" --output "$3"
  valid "$2" "$3" || { echo 'NSIS tool checksum mismatch' >&2; exit 1; }
}
exe_digest=78e17de55fa013be7620487e85b6676c204c6c51d7ebabb74ee8a9cfce6252a0
if ! valid "$exe_digest" "$cache/NSIS/NSIS.exe" 2>/dev/null; then
  fetch https://api.github.com/repos/tauri-apps/binary-releases/releases/assets/623832696 \
    ba63dffc4410ee89193e1cb5a41989991bd77c61068da17e3156d136b7b0b3d8 "$stage/nsis.zip"
  python3 - "$stage" <<'PY'
import pathlib, sys, zipfile
stage = pathlib.Path(sys.argv[1])
with zipfile.ZipFile(stage / 'nsis.zip') as archive:
    (stage / 'NSIS.exe').write_bytes(archive.read('nsis-3.13/NSIS.exe'))
PY
  valid "$exe_digest" "$stage/NSIS.exe"
  mv "$stage/NSIS.exe" "$cache/NSIS/NSIS.exe"
fi
dll="$cache/NSIS/Plugins/x86-unicode/additional/nsis_tauri_utils.dll"
dll_digest=5ba143b5db4a87d32d6e7802e033330aae56cbceabe0d1e3ba41948385ad4709
if ! valid "$dll_digest" "$dll" 2>/dev/null; then
  fetch https://api.github.com/repos/tauri-apps/nsis-tauri-utils/releases/assets/345882097 \
    "$dll_digest" "$stage/nsis_tauri_utils.dll"
  mv "$stage/nsis_tauri_utils.dll" "$dll"
fi
