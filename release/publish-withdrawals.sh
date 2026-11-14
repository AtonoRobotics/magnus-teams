#!/bin/sh
# Publish the catalog's signed withdrawal list beside the server's update
# feed (decisions/2026-11-13-catalog-withdrawal.md). Run by the release
# workflow on the server, where the release key is; run it there by hand to
# withdraw a connector without waiting for a release.
#
#   release/publish-withdrawals.sh UPDATES_DIR [LIST]
#
# LIST is catalog/withdrawn.json unless named: {"serial": N, "withdrawn":
# [{"id": "slack", "versions": "<1.4.0", "reason": "..."}]}, `versions` a
# semver requirement (none: every version). Every change to it takes a higher
# serial: the server refuses a list older than the one it applied, so an old
# signed list cannot be replayed to bring a connector back. The same serial
# with the same entries is already published and left as it is; the same
# serial with other entries is refused here.
#
# The list is signed with `tauri signer sign --app-version
# catalog-withdrawals.N`, as a release package is, so the signature binds its
# serial and the server verifies it with the release key it already has
# (`update::verify`). The signature is written first and the list last, each
# by rename: a server that reads between them finds the pair does not verify
# and keeps its last good list until its next look.
set -eu

updates=$1
list=${2:-catalog/withdrawn.json}
key=${MAGNUS_RELEASE_KEY:-$HOME/.magnus-teams-release/update.key}
[ -r "$key" ] || { echo "no release key at $key" >&2; exit 1; }
[ -r "$list" ] || { echo "no withdrawal list at $list" >&2; exit 1; }
dir=$updates/catalog
mkdir -p "$dir"

# Checks the list and compares it with the one published: prints its serial,
# or exits 10 when it is already published.
serial=$(python3 -I - "$list" "$dir/withdrawn.json" <<'PY'
import json, re, sys
new = json.load(open(sys.argv[1]))
serial = new.get("serial")
if not isinstance(serial, int) or isinstance(serial, bool) or serial < 1:
    sys.exit("the list's serial is a whole number from 1")
entries = new.get("withdrawn", [])
comparator = re.compile(r"^\s*(=|>=|<=|>|<|~|\^)?\s*\d+(\.\d+){0,2}(-[0-9A-Za-z.-]+)?\s*$")
seen = set()
for w in entries:
    if not isinstance(w, dict) or not isinstance(w.get("id"), str) or not w["id"]:
        sys.exit("each withdrawn entry names its connector's id")
    if w["id"] in seen:
        sys.exit(f"{w['id']} is withdrawn twice")
    seen.add(w["id"])
    v = w.get("versions")
    if v is not None and not (isinstance(v, str) and all(comparator.match(p) for p in v.split(","))):
        sys.exit(f"{w['id']}: versions is a semver requirement, such as <1.4.0")
try:
    old = json.load(open(sys.argv[2]))
except FileNotFoundError:
    old = None
if old is not None:
    if serial < old["serial"]:
        sys.exit(f"serial {serial} is older than the published {old['serial']}")
    if serial == old["serial"]:
        if old.get("withdrawn", []) != entries:
            sys.exit(f"the list changed and its serial is still {serial}: raise it")
        sys.exit(10)
print(serial)
PY
) || {
  status=$?
  if [ "$status" = 10 ]; then
    echo "the withdrawal list is already published"
    exit 0
  fi
  exit "$status"
}

stage=$(mktemp -d "$dir/.stage.XXXXXX")
trap 'rm -rf "$stage"' EXIT
cp "$list" "$stage/withdrawn.json"
env -u TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
  cargo tauri signer sign -f "$key" -p "" --app-version "catalog-withdrawals.$serial" "$stage/withdrawn.json" >/dev/null
[ -s "$stage/withdrawn.json.sig" ] || { echo "no signature for the withdrawal list" >&2; exit 4; }
mv "$stage/withdrawn.json.sig" "$dir/withdrawn.json.sig"
mv "$stage/withdrawn.json" "$dir/withdrawn.json"
echo "published the withdrawal list, serial $serial, to $dir"
