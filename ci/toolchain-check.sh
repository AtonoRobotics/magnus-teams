#!/bin/sh
# Checks that the checks run on one Node and in one timezone everywhere
# (decisions/2026-11-20-one-node-and-one-timezone.md):
#   - ci/fast.sh refuses a Node other than .node-version's before it checks
#     anything, and runs every check with TZ=Pacific/Kiritimati;
#   - ci/ensure-node.sh has a digest for .node-version's version;
#   - fast.yml, the release build and ci/on-strix-beta.sh put that Node first.
# Uses a stand-in node and cargo, so nothing is built or downloaded.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
fake=$(mktemp -d)
trap 'rm -rf "$fake"' EXIT
fail=0
bad() { echo "FAIL: $*" >&2; fail=1; }
want=$(tr -d ' \n' < "$root/.node-version")

# A stand-in cargo notes the timezone it ran in, then fails: the first check
# ci/fast.sh runs is `cargo fmt`, so the run ends there.
printf '#!/bin/sh\necho "TZ=${TZ:-unset}" > "%s/cargo-ran"\nexit 1\n' "$fake" > "$fake/cargo"
chmod +x "$fake/cargo"
node_says() { printf '#!/bin/sh\necho %s\n' "$1" > "$fake/node"; chmod +x "$fake/node"; }

node_says v18.19.1
rm -f "$fake/cargo-ran"
if PATH="$fake:$PATH" sh "$root/ci/fast.sh" > "$fake/out" 2>&1; then bad "ci/fast.sh passed on Node 18"; fi
[ ! -e "$fake/cargo-ran" ] || bad "ci/fast.sh ran a check on Node 18 instead of stopping first"
grep -q "Node v$want" "$fake/out" || bad "ci/fast.sh on the wrong Node does not name v$want: $(cat "$fake/out")"

node_says "v$want"
rm -f "$fake/cargo-ran"
PATH="$fake:$PATH" sh "$root/ci/fast.sh" > "$fake/out" 2>&1 || true
if [ -e "$fake/cargo-ran" ]; then
  [ "$(cat "$fake/cargo-ran")" = "TZ=Pacific/Kiritimati" ] || bad "ci/fast.sh ran its checks with $(cat "$fake/cargo-ran")"
else
  bad "ci/fast.sh on Node v$want did not reach its checks: $(cat "$fake/out")"
fi

grep -q "^  $want) digest=[0-9a-f]\{64\} ;;" "$root/ci/ensure-node.sh" || bad "ci/ensure-node.sh has no digest for Node $want"
step='run: sh ci/ensure-node.sh >> "$GITHUB_PATH"'
grep -qF "$step" "$root/.github/workflows/fast.yml" || bad "fast.yml does not put the pinned Node first"
# The release build puts it first before anything runs Node: its checks and npm.
awk -v step="$step" 'index($0, step) { seen = 1 } /name: (Fast checks|Images)/ && !seen { exit 1 }' "$root/.github/workflows/release.yml" \
  && grep -qF "$step" "$root/.github/workflows/release.yml" || bad "the release build does not put the pinned Node first"
grep -qF 'node_bin=\$(sh ci/ensure-node.sh) && export PATH=\$node_bin:\$PATH' "$root/ci/on-strix-beta.sh" || bad "ci/on-strix-beta.sh does not put the pinned Node first"

[ "$fail" = 0 ] && echo "toolchain check passed"
exit "$fail"
