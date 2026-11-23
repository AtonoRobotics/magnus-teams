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

# One Node and one timezone on every machine that runs these
# (decisions/2026-11-20-one-node-and-one-timezone.md). The zone is UTC+14
# with no daylight saving: its date differs from UTC's for 14 hours of each
# day, so a check that mixes a UTC date with a local one fails on most runs,
# on every machine alike, instead of passing on one and failing on another.
export TZ=Pacific/Kiritimati
want=v$(tr -d ' \n' < .node-version)
have=$(node --version 2>/dev/null || echo none)
if [ "$have" != "$want" ]; then
  echo "::error::the checks run on Node $want (.node-version), and this is $have; put it first: PATH=\"\$(sh ci/ensure-node.sh):\$PATH\""
  exit 1
fi

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
# Cargo.lock as committed, as the release build fetches it: a lock file that
# Cargo.toml has moved past fails here, not later in the release.
check cargo fetch --locked
check sh ci/production-guard-check.sh
check sh ci/strix-gc-check.sh
check sh ci/ratchet-check.sh
check sh ci/release-gate-check.sh
check sh ci/toolchain-check.sh
check sh ci/egress-check.sh
check python3 -I sandbox/desktop/test_desktop.py
check python3 -I sandbox/test_images.py
for f in desktop/tests/*-check.mjs; do
  check node "$f"
done
check sh ci/ratchet.sh size
check sh ci/ratchet.sh clippy
check cargo test --workspace --lib --bins
