#!/bin/sh
# Checks that a commit is released only after its fast run passed
# (decisions/2026-11-20-release-waits-for-fast.md):
#   - release.yml starts when the "fast" workflow completes on main, not on
#     the push, and builds only when that run passed;
#   - release.yml checks out the commit the fast run checked;
#   - fast.yml and a manual release run the same script, ci/fast.sh;
#   - ci/fast.sh runs every desktop/tests/*-check.mjs and every ci/*-check.sh.
# It reads the files only; nothing is built or run.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
release=$root/.github/workflows/release.yml
fast=$root/.github/workflows/fast.yml
script=$root/ci/fast.sh
fail=0
bad() { echo "FAIL: $*" >&2; fail=1; }
has() { grep -qF -- "$2" "$1"; }

# The workflow's trigger block: from `on:` to the next top-level key.
triggers=$(awk '/^on:/ { on = 1; next } on && /^[^ #]/ { exit } on' "$release")
case $triggers in *"push:"*) bad "release.yml starts on a push; it must wait for the fast run" ;; esac
for want in "workflow_run:" "workflows: [fast]" "types: [completed]" "branches: [main]"; do
  case $triggers in *"$want"*) ;; *) bad "release.yml's trigger lacks '$want'" ;; esac
done

# The build job runs only for a manual run or a fast run that passed.
build_if=$(awk '/^  build:/ { b = 1; next } b && /^  [a-z]/ { exit } b && /^    if:/ { i = 1 } i && /^    [a-z-]+:/ && !/^    if:/ { exit } i' "$release")
case $build_if in *"github.event.workflow_run.conclusion == 'success'"*) ;; *) bad "release.yml's build job does not require the fast run to have passed" ;; esac
case $build_if in *"github.event.workflow_run.event == 'push'"*) ;; *) bad "release.yml's build job does not require the fast run to be for a push" ;; esac

# Every checkout is of the commit the fast run checked.
checkouts=$(grep -c 'uses: actions/checkout@' "$release")
refs=$(grep -cF 'ref: ${{ github.event.workflow_run.head_sha || github.sha }}' "$release")
[ "$checkouts" -gt 0 ] && [ "$checkouts" -eq "$refs" ] || bad "release.yml has $checkouts checkouts and $refs of the fast run's commit"

# One list of checks.
grep -qx 'name: fast' "$fast" || bad "fast.yml is not named 'fast', which release.yml waits for"
has "$fast" 'run: sh ci/fast.sh' || bad "fast.yml does not run ci/fast.sh"
others=$(grep -E '^[[:space:]]*(- )?run:' "$fast" | grep -vF 'sh ci/fast.sh' | grep -vF 'rustup show' | grep -vF 'sh ci/ensure-node.sh' || true)
[ -z "$others" ] || bad "fast.yml runs checks outside ci/fast.sh: $others"
has "$release" 'run: sh ci/fast.sh' || bad "a manual release does not run ci/fast.sh"
has "$script" 'for f in desktop/tests/*-check.mjs; do' || bad "ci/fast.sh does not run every desktop/tests/*-check.mjs"
# The lock file is checked as committed before any cargo command could
# rewrite it, as the release build's `cargo fetch --locked` reads it.
first_cargo=$(grep -E '^check cargo ' "$script" | grep -v 'cargo fmt' | head -1)
[ "$first_cargo" = "check cargo fetch --locked" ] || bad "ci/fast.sh does not check Cargo.lock with 'cargo fetch --locked' before its other cargo commands (first is: $first_cargo)"
grep -qF 'cargo fetch --locked' "$release" || bad "the release build no longer fetches with --locked"
for f in "$root"/ci/*-check.sh; do
  n=ci/$(basename "$f")
  has "$script" "sh $n" || bad "ci/fast.sh does not run $n"
done

[ "$fail" = 0 ] && echo "release gate check passed"
exit "$fail"
