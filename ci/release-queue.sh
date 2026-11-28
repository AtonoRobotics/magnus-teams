#!/bin/sh
# The release queue (decisions/2026-11-28-release-queue.md). The first job of
# every release run. Decides whether this run builds and publishes, and
# writes release=true or release=false to $GITHUB_OUTPUT:
#
#   1. A commit whose fast run did not pass is not released.
#   2. Otherwise the run waits until every release run created before it has
#      completed. No release run cancels or replaces another, and two never
#      build or publish at once. Fast runs complete in push order
#      (decisions/2026-11-27-fast-runs-fifo.md), and each starts its release
#      run, so the release runs wait in push order too.
#   3. When its turn comes, a fast run's release is superseded if a later
#      push's fast run on the same branch has already passed: that later
#      release holds this commit too and comes next, so this one completes,
#      successfully, without building something about to be replaced. A
#      manual run is never superseded.
#
# Usage: ci/release-queue.sh REPOSITORY RUN_ID EVENT FAST_RUN_ID FAST_CONCLUSION
# EVENT is workflow_run or workflow_dispatch; for a manual run, FAST_RUN_ID
# and FAST_CONCLUSION are empty. Needs GH_TOKEN with actions: read.
set -eu
# The test (ci/release-queue-check.sh) polls faster.
POLL_INTERVAL_SECONDS=${RELEASE_QUEUE_POLL_INTERVAL_SECONDS:-30}
WAIT_LIMIT_SECONDS=14400

repo=$1 run=$2 event=$3 fast_run=${4:-} fast_conclusion=${5:-}
out=${GITHUB_OUTPUT:-/dev/stdout}
summary=${GITHUB_STEP_SUMMARY:-/dev/stderr}
decide() { echo "release=$1" >> "$out"; echo "$2" | tee -a "$summary" >&2; exit 0; }

if [ "$event" != workflow_dispatch ] && [ "$fast_conclusion" != success ]; then
  decide false "Not released: this commit's fast run ended '$fast_conclusion'."
fi

# 2. Wait for every earlier release run. Run ids grow with creation time.
waited=0
while :; do
  ahead=$(gh api --paginate "repos/$repo/actions/workflows/release.yml/runs?per_page=100&created=>=$(date -u -d '-1 day' +%Y-%m-%dT%H:%M:%SZ)" \
    --jq ".workflow_runs[] | select(.id < $run and .status != \"completed\") | .id" | sort -n | tr '\n' ' ')
  [ -n "$ahead" ] || break
  if [ "$waited" -ge "$WAIT_LIMIT_SECONDS" ]; then
    echo "::error::release runs $ahead were still not complete after $WAIT_LIMIT_SECONDS s"
    exit 1
  fi
  echo "waiting for earlier release runs: $ahead" >&2
  sleep "$POLL_INTERVAL_SECONDS"
  waited=$((waited + POLL_INTERVAL_SECONDS))
done

[ "$event" != workflow_dispatch ] || decide true "Releasing: a manual run."

# 3. Superseded by a later push whose fast run already passed?
branch=$(gh api "repos/$repo/actions/runs/$fast_run" --jq .head_branch)
newer=$(gh api "repos/$repo/actions/workflows/fast.yml/runs?branch=$branch&event=push&status=success&per_page=50" \
  --jq "[.workflow_runs[] | select(.id > $fast_run)] | max_by(.id) | .head_sha // empty")
if [ -n "$newer" ]; then
  decide false "Superseded: the later commit $newer has passed its fast run, and its release, next in the queue, includes this commit."
fi
decide true "Releasing: no later commit has passed its fast run yet."
