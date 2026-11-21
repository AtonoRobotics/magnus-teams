#!/bin/sh
# The fast tier's checks, in one place: .github/workflows/fast.yml runs this
# on every push, and a release starts only after it passed for that commit
# (decisions/2026-11-20-release-waits-for-fast.md). A manual release runs it
# too. Every desktop/tests/*-check.mjs runs, found by name, so a new check
# cannot be left out of a hand-kept list.
#
# Cheap checks first, the build last. Each check is a group in the Actions
# log, and a failure names the check that failed.
#
# Run it on Strix Beta, never on the production server:
#   ci/on-strix-beta.sh NAME sh ci/fast.sh
set -eu
cd "$(dirname "$0")/.."

check() {
  echo "::group::$*"
  if "$@"; then
    echo "::endgroup::"
  else
    status=$?
    echo "::endgroup::"
    echo "::error::fast check failed (exit $status): $*"
    exit "$status"
  fi
}

check cargo fmt --all --check
check sh ci/production-guard-check.sh
check sh ci/strix-gc-check.sh
check sh ci/ratchet-check.sh
check sh ci/release-gate-check.sh
check python3 -I sandbox/desktop/test_desktop.py
check python3 -I sandbox/test_images.py
for f in desktop/tests/*-check.mjs; do
  check node "$f"
done
check sh ci/ratchet.sh size
check sh ci/ratchet.sh clippy
check cargo test --workspace --lib --bins
