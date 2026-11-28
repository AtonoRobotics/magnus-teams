#!/bin/sh
# Puts a commit status on a commit, so a commit that came in a push with
# others shows its own verdict in the repository (fast-earlier.yml;
# decisions/2026-11-28-check-every-pushed-commit.md).
# Usage: ci/commit-status.sh REPOSITORY SHA STATE DESCRIPTION
# STATE is pending, success, failure or error. Needs GH_TOKEN with
# statuses: write; RUN_URL, when set, is what the status links to.
set -eu
repo=${1:?repository}
sha=${2:?sha}
state=${3:?state}
description=${4:?description}
gh api -X POST "repos/$repo/statuses/$sha" \
  -f state="$state" \
  -f context="fast-earlier" \
  -f description="$description" \
  -f target_url="${RUN_URL:-}" > /dev/null
