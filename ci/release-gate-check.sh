#!/bin/sh
# Checks that a commit is released only after its fast run passed
# (decisions/2026-11-20-release-waits-for-fast.md):
#   - release.yml starts when the "fast" workflow completes on main, not on
#     the push, and builds only when that run passed;
#   - release.yml checks out the commit the fast run checked;
#   - fast.yml runs are a FIFO queue: one per push, none cancelled or
#     replaced, completing in push order;
#   - fast.yml and a manual release run the same script, ci/fast.sh;
#   - fast.yml runs both its parts, the checks and the desktop app, and the
#     in-order job waits for both;
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

# Fast runs are a first-in, first-out queue: each push has a group of its own
# commit, so none is cancelled or replaced, and the in-order job makes runs
# complete in push order (decisions/2026-11-27-fast-runs-fifo.md).
fast_conc=$(awk '/^concurrency:/ { c = 1; next } c && /^[^ #]/ { exit } c' "$fast")
case $fast_conc in *'group: fast-${{ github.ref }}-${{ github.sha }}'*) ;; *) bad "fast.yml's concurrency group is not one per commit, so a newer push can replace a waiting one" ;; esac
case $fast_conc in *"cancel-in-progress: false"*) ;; *) bad "fast.yml lets a newer push cancel the run in progress" ;; esac
in_order=$(awk '/^  in-order:/ { j = 1; next } j && /^  [a-z]/ { exit } j' "$fast")
case $in_order in *"needs: [fast, desktop]"*) ;; *) bad "fast.yml has no in-order job after the fast and desktop jobs" ;; esac
case $in_order in *"if: always()"*) ;; *) bad "fast.yml's in-order job does not run when the checks fail" ;; esac
case $in_order in *'sh ci/fast-wait-previous.sh "$REPOSITORY" "$PREVIOUS_SHA"'*) ;; *) bad "fast.yml's in-order job does not wait for the previous push's run" ;; esac
case $in_order in *'PREVIOUS_SHA: ${{ github.event.before }}'*) ;; *) bad "fast.yml's in-order job does not wait on the commit before the push" ;; esac

# The commits of a push before its tip are checked by fast-earlier.yml, which
# is not a gate: release.yml waits for "fast" alone, so a commit that fails
# there neither releases nor holds up another push
# (decisions/2026-11-28-check-every-pushed-commit.md). It runs all of
# ci/fast.sh, as fast.yml does, and takes no more permission than a status.
earlier=$root/.github/workflows/fast-earlier.yml
[ -f "$earlier" ] || bad "fast-earlier.yml is missing, so commits before a push's tip have no check"
if [ -f "$earlier" ]; then
  grep -qx 'name: fast-earlier' "$earlier" || bad "fast-earlier.yml is not named 'fast-earlier'; named 'fast', release.yml would wait for it"
  grep -qx 'on:' "$earlier" && has "$earlier" "  push:" || bad "fast-earlier.yml does not run on a push"
  has "$earlier" "run: sh ci/fast.sh" || bad "fast-earlier.yml does not run ci/fast.sh on each commit"
  has "$earlier" "fail-fast: false" || bad "fast-earlier.yml lets one commit's failure cancel the others' checks"
  has "$earlier" "statuses: write" || bad "fast-earlier.yml cannot put a status on a commit"
  has "$earlier" "ref: \${{ matrix.sha }}" || bad "fast-earlier.yml does not check out each commit it checks"
  case $release in *) grep -qF 'fast-earlier' "$release" && bad "release.yml mentions fast-earlier; it must wait for fast alone" ;; esac
fi

# One list of checks.
grep -qx 'name: fast' "$fast" || bad "fast.yml is not named 'fast', which release.yml waits for"
has "$fast" 'run: sh ci/fast.sh checks' || bad "fast.yml does not run ci/fast.sh's checks"
has "$fast" 'run: sh ci/fast.sh desktop' || bad "fast.yml does not compile and test the desktop app (ci/fast.sh desktop)"
others=$(grep -E '^[[:space:]]*(- )?run:' "$fast" | grep -vF 'sh ci/fast.sh' | grep -vF 'rustup show' | grep -vF 'sh ci/ensure-node.sh' | grep -vF 'sh ci/desktop-libs.sh' | grep -vF 'sh ci/fast-wait-previous.sh' || true)
[ -z "$others" ] || bad "fast.yml runs checks outside ci/fast.sh: $others"
has "$release" 'run: sh ci/fast.sh' || bad "a manual release does not run ci/fast.sh"
has "$script" 'for f in desktop/tests/*-check.mjs; do' || bad "ci/fast.sh does not run every desktop/tests/*-check.mjs"
has "$script" 'check cargo test --locked --manifest-path "$m" --lib --bins' || bad "ci/fast.sh does not compile and test the desktop app"
has "$script" 'check sh ci/ratchet.sh clippy-desktop' || bad "ci/fast.sh does not hold the desktop app's clippy warnings to their baseline"
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
