#!/bin/sh
# Makes fast runs finish in push order (decisions/2026-11-27-fast-runs-fifo.md).
# Every push gets its own fast run, and the runs check in parallel. This step,
# the last job of fast.yml, waits until the fast run for the push before this
# one (the commit the branch pointed to before it) has completed, whatever
# its verdict. Each run waits for the one before it, so runs complete in the
# order they were pushed, and release.yml, which starts on a fast run's
# completion, releases commits in that order too.
#
# Usage: ci/fast-wait-previous.sh REPOSITORY PREVIOUS_SHA
# Needs GH_TOKEN with actions: read. Nothing to wait for (a new branch, a
# manual run, a force push whose old commit had no run) returns at once.
set -eu
POLL_INTERVAL_SECONDS=20
WAIT_LIMIT_SECONDS=1800

repo=${1:?repository}
previous=${2:-}
case $previous in "" | 0000000000000000000000000000000000000000)
  echo "no previous push to wait for"; exit 0 ;;
esac

waited=0
while :; do
  # The newest fast run for the previous push; empty when it has none.
  status=$(gh api "repos/$repo/actions/workflows/fast.yml/runs?head_sha=$previous&event=push&per_page=1" \
    --jq '.workflow_runs[0].status // ""')
  case $status in
    "") echo "no fast run for $previous; nothing to wait for"; exit 0 ;;
    completed) echo "fast run for $previous has completed"; exit 0 ;;
  esac
  if [ "$waited" -ge "$WAIT_LIMIT_SECONDS" ]; then
    echo "::error::the fast run for $previous was still $status after $WAIT_LIMIT_SECONDS s"
    exit 1
  fi
  echo "waiting for the fast run for $previous ($status)"
  sleep "$POLL_INTERVAL_SECONDS"
  waited=$((waited + POLL_INTERVAL_SECONDS))
done
