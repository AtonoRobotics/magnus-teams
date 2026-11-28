#!/bin/sh
# The release queue (decisions/2026-11-28-release-queue.md). The first job of
# every release run. Decides whether this run builds and publishes, and
# writes release=true or release=false to $GITHUB_OUTPUT:
#
#   1. A commit whose fast run did not pass is not released.
#   2. Every other commit is released, in commit order: the run waits until
#      every release run created before it has completed, then builds and
#      publishes. No release run cancels, replaces or skips another, and two
#      never build or publish at once. Fast runs complete in push order
#      (decisions/2026-11-27-fast-runs-fifo.md), and each starts its release
#      run, so the release runs wait, and release, in push order too.
#
# Usage: ci/release-queue.sh REPOSITORY RUN_ID EVENT FAST_RUN_ID FAST_CONCLUSION
# EVENT is workflow_run or workflow_dispatch; for a manual run, FAST_RUN_ID
# and FAST_CONCLUSION are empty. FAST_RUN_ID is kept for the log. Needs GH_TOKEN with actions: read.
set -eu
# The test (ci/release-queue-check.sh) polls faster.
POLL_INTERVAL_SECONDS=${RELEASE_QUEUE_POLL_INTERVAL_SECONDS:-30}
# Under the hosted runner's six-hour job limit (release.yml's queue job).
WAIT_LIMIT_SECONDS=19800

repo=$1 run=$2 event=$3 fast_conclusion=${5:-}
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

if [ "$event" = workflow_dispatch ]; then
  decide true "Releasing: a manual run, its turn in the queue."
fi
decide true "Releasing: this commit's turn in the queue."
