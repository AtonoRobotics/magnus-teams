#!/bin/sh
# Checks the release queue (decisions/2026-11-28-release-queue.md):
# ci/release-queue.sh's decisions, against a fake `gh` that plays GitHub's
# API, and release.yml's wiring: no concurrency group to cancel a waiting
# release, a queue job first, and a build only on the queue's word.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
queue=$root/ci/release-queue.sh
release=$root/.github/workflows/release.yml
fail=0
bad() { echo "FAIL: $*" >&2; fail=1; }
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# The fake gh answers from files: $tmp/runs.json (release runs, one JSON
# document per call, the last repeated), $tmp/fast.json (fast runs) and
# $tmp/branch. It applies the caller's --jq with jq, as gh does.
mkdir -p "$tmp/bin"
cat > "$tmp/bin/gh" <<'EOF'
#!/bin/sh
url= q=
while [ $# -gt 0 ]; do
  case $1 in --jq) q=$2; shift ;; --paginate|api) ;; *) url=$1 ;; esac
  shift
done
d=$FAKE_GH
case $url in
  */workflows/release.yml/runs*)
    n=$(cat "$d/calls" 2>/dev/null || echo 0); n=$((n + 1)); echo $n > "$d/calls"
    f=$d/runs.$n.json; [ -f "$f" ] || f=$(ls "$d"/runs.*.json | sort -t. -k2 -n | tail -1)
    jq -r "$q" "$f" ;;
  */workflows/fast.yml/runs*) jq -r "$q" "$d/fast.json" ;;
  */actions/runs/*) echo main ;;
  *) echo "fake gh: unexpected $url" >&2; exit 9 ;;
esac
EOF
chmod +x "$tmp/bin/gh"

# run NAME EVENT FAST_RUN FAST_CONCLUSION: prints the decision (true/false).
run() {
  d=$tmp/$1; out=$d/out
  FAKE_GH=$d PATH=$tmp/bin:$PATH RELEASE_QUEUE_POLL_INTERVAL_SECONDS=0 \
    GITHUB_OUTPUT=$out GITHUB_STEP_SUMMARY=$d/summary \
    sh "$queue" o/r 500 "$2" "$3" "$4" 2>/dev/null || { echo "error"; return; }
  sed -n 's/^release=//p' "$out"
}
case_() { mkdir -p "$tmp/$1"; }
no_runs='{"workflow_runs":[{"id":500,"status":"in_progress"}]}'
fast_none='{"workflow_runs":[{"id":100,"head_sha":"aaa"}]}'
fast_newer='{"workflow_runs":[{"id":100,"head_sha":"aaa"},{"id":120,"head_sha":"ccc"},{"id":110,"head_sha":"bbb"}]}'

# A failed fast run is not released, without waiting.
case_ failed; echo "$no_runs" > "$tmp/failed/runs.1.json"; echo "$fast_none" > "$tmp/failed/fast.json"
[ "$(run failed workflow_run 100 failure)" = false ] || bad "a commit whose fast run failed is released"
[ ! -f "$tmp/failed/calls" ] || bad "a commit whose fast run failed waited in the queue"

# A passed commit with nothing after it is released.
case_ alone; echo "$no_runs" > "$tmp/alone/runs.1.json"; echo "$fast_none" > "$tmp/alone/fast.json"
[ "$(run alone workflow_run 100 success)" = true ] || bad "a passed commit with no later commit is not released"

# It waits until earlier release runs complete, and none later counts.
case_ waits
echo '{"workflow_runs":[{"id":480,"status":"in_progress"},{"id":490,"status":"queued"},{"id":500,"status":"in_progress"},{"id":510,"status":"queued"}]}' > "$tmp/waits/runs.1.json"
echo '{"workflow_runs":[{"id":480,"status":"completed"},{"id":490,"status":"in_progress"},{"id":500,"status":"in_progress"},{"id":510,"status":"queued"}]}' > "$tmp/waits/runs.2.json"
echo '{"workflow_runs":[{"id":480,"status":"completed"},{"id":490,"status":"completed"},{"id":500,"status":"in_progress"},{"id":510,"status":"queued"}]}' > "$tmp/waits/runs.3.json"
echo "$fast_none" > "$tmp/waits/fast.json"
[ "$(run waits workflow_run 100 success)" = true ] || bad "a release behind earlier runs is not released once they complete"
[ "$(cat "$tmp/waits/calls")" = 3 ] || bad "the queue did not wait for every earlier release run (asked $(cat "$tmp/waits/calls") times, not 3)"

# A later commit that passed supersedes it, and the newest one is named.
case_ superseded; echo "$no_runs" > "$tmp/superseded/runs.1.json"; echo "$fast_newer" > "$tmp/superseded/fast.json"
[ "$(run superseded workflow_run 100 success)" = false ] || bad "a commit whose later commit passed is built anyway"
grep -q 'ccc' "$tmp/superseded/summary" || bad "a superseded release does not name the newest passed commit"

# A manual run is released after waiting, never superseded.
case_ manual; echo "$no_runs" > "$tmp/manual/runs.1.json"; echo "$fast_newer" > "$tmp/manual/fast.json"
[ "$(run manual workflow_dispatch '' '')" = true ] || bad "a manual release run is not released"

# release.yml's wiring.
! grep -q '^concurrency:' "$release" || bad "release.yml has a concurrency group, which cancels a waiting release"
grep -q '^  queue:' "$release" || bad "release.yml has no queue job"
grep -qF 'run: sh ci/release-queue.sh "$REPOSITORY" "$RUN_ID" "$EVENT" "$FAST_RUN_ID" "$FAST_CONCLUSION"' "$release" || bad "release.yml's queue job does not run ci/release-queue.sh"
build=$(awk '/^  build:/ { b = 1; next } b && /^  [a-z]/ { exit } b' "$release")
case $build in *"needs: queue"*) ;; *) bad "release.yml's build does not wait for the queue job" ;; esac
case $build in *"needs.queue.outputs.release == 'true'"*) ;; *) bad "release.yml builds without the queue's word" ;; esac

[ "$fail" = 0 ] && echo "release queue check passed"
exit "$fail"
