#!/bin/sh
# Checks that ci/ratchet.sh clippy fails closed: a cargo that writes nothing,
# writes garbage, or never finishes its build is a failure, not zero
# warnings; and a warning without a place in the code is still counted.
# Uses a stand-in cargo, so nothing is built.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
fake=$(mktemp -d)
trap 'rm -rf "$fake"' EXIT
fail=0
run() {
  want=$1 name=$2 body=$3
  printf '#!/bin/sh\n%s\n' "$body" > "$fake/cargo"
  chmod +x "$fake/cargo"
  if PATH="$fake:$PATH" sh "$root/ci/ratchet.sh" clippy > "$fake/out" 2>&1; then got=pass; else got=fail; fi
  if [ "$got" != "$want" ]; then echo "FAIL: $name: want $want, got $got" >&2; cat "$fake/out" >&2; fail=1; fi
}
finished='echo "{\"reason\":\"build-finished\",\"success\":true}"'
spanless='echo "{\"reason\":\"compiler-message\",\"message\":{\"level\":\"warning\",\"message\":\"spanless %s\",\"code\":null,\"spans\":[]}}"'
base=$(cat "$root/ci/clippy-baseline")
run fail "no output" 'exit 0'
run fail "cargo missing or broken" 'exit 101'
run fail "garbage" 'echo garbage'
run fail "build not finished" 'echo "{\"reason\":\"build-finished\",\"success\":false}"'
run fail "finished, but cargo failed" "$finished; exit 1"
run pass "a clean build" "$finished"
# One more spanless warning than the baseline allows: each counts.
many=$(i=0; while [ "$i" -le "$base" ]; do printf '%s\n' "$spanless" | sed "s/%s/$i/"; i=$((i + 1)); done)
run fail "spanless warnings over the baseline" "$finished
$many"
[ $fail = 0 ] && echo "ratchet: ok"
exit $fail
