#!/bin/sh
# Checks that ci/ratchet.sh clippy fails closed: a cargo that writes nothing,
# writes garbage, or never finishes its build is a failure, not zero
# warnings; and a warning without a place in the code is still counted.
# Uses a stand-in cargo, so nothing is built. The desktop app's clippy
# ratchet is the same code with its own baseline, checked once here too.
#
# And that ci/ratchet.sh size refuses a file over 800 lines, in any crate,
# when ci/oversize.txt is empty as well as when it lists others, in a
# throwaway repository.
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
run_desktop() {
  want=$1 name=$2 body=$3
  printf '#!/bin/sh\n%s\n' "$body" > "$fake/cargo"
  chmod +x "$fake/cargo"
  if PATH="$fake:$PATH" sh "$root/ci/ratchet.sh" clippy-desktop > "$fake/out" 2>&1; then got=pass; else got=fail; fi
  if [ "$got" != "$want" ]; then echo "FAIL: desktop $name: want $want, got $got" >&2; cat "$fake/out" >&2; fail=1; fi
}
run_desktop fail "no output" 'exit 0'
run_desktop pass "a clean build" "$finished"
desktop_base=$(cat "$root/ci/clippy-baseline-desktop")
many=$(i=0; while [ "$i" -le "$desktop_base" ]; do printf '%s\n' "$spanless" | sed "s/%s/$i/"; i=$((i + 1)); done)
run_desktop fail "spanless warnings over the baseline" "$finished
$many"

# size: FILES is "path:lines ...", OVERSIZE the baseline's lines.
size() {
  want=$1 name=$2 files=$3 oversize=$4
  repo=$fake/size
  rm -rf "$repo"
  mkdir -p "$repo/ci"
  cp "$root/ci/ratchet.sh" "$repo/ci/"
  printf '%s' "$oversize" > "$repo/ci/oversize.txt"
  for f in $files; do
    path=${f%:*} lines=${f##*:}
    mkdir -p "$repo/$(dirname "$path")"
    yes '//' | head -n "$lines" > "$repo/$path"
  done
  git -C "$repo" init -q
  if sh "$repo/ci/ratchet.sh" size > "$fake/out" 2>&1; then got=pass; else got=fail; fi
  if [ "$got" != "$want" ]; then echo "FAIL: size $name: want $want, got $got" >&2; cat "$fake/out" >&2; fail=1; fi
}
size pass "every file at the limit" "crates/a/src/lib.rs:800 desktop/src-tauri/src/lib.rs:800" ""
size fail "a desktop file over the limit, empty baseline" "desktop/src-tauri/src/lib.rs:801" ""
size fail "a kernel file over the limit, empty baseline" "crates/a/src/lib.rs:801" ""
size fail "a new file over the limit beside a listed one" "crates/a/src/lib.rs:900 crates/b/src/lib.rs:801" "crates/a/src/lib.rs 900
"
size pass "a listed file at its baseline" "crates/a/src/lib.rs:900" "crates/a/src/lib.rs 900
"
size fail "a listed file that grew" "crates/a/src/lib.rs:901" "crates/a/src/lib.rs 900
"
size pass "a test file over the limit" "desktop/src-tauri/src/lib_tests.rs:2000 crates/a/tests/t.rs:2000" ""

[ $fail = 0 ] && echo "ratchet: ok"
exit $fail
