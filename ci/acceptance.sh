#!/bin/sh
# Acceptance: the heavy suites a release candidate runs before it ships
# (AGENTS.md, "How work lands"). Never a gate on a push; fast.yml is that.
# A release that fails one does not ship. Run it on Strix Beta:
#
#   ci/on-strix-beta.sh acceptance ci/acceptance.sh            every suite
#   ci/on-strix-beta.sh acceptance ci/acceptance.sh autonomy   one suite
#
# Suites:
#   autonomy    the eight autonomy scenarios (spec rev 133, 140 and 142,
#               "Autonomy is tested like safety"), tests/scratch/tests/autonomy.rs;
#               the eighth, the agent using its computer with nobody having set
#               one up, needs computerd's KVM and guest images
#   injection   prompt injection, tests/scratch/tests/injection.rs
#   update      signed updates, tests/scratch/tests/update.rs
#   invariants  the eight invariants in full, tests/invariants
#   kill_switch_computer  each kill switch level pausing a program running
#               in the computer until release (spec rev 141), needs KVM
#   desktop_look  the computer's desktop screenshotted in dark and light
#               and compared with its mockup, and windows placed calmly
#               (spec rev 145), needs KVM and the guest image built from
#               this checkout (sandbox/image.sh); MAGNUS_DESKTOP_SHOTS=DIR
#               keeps the screenshots
set -eu
cd "$(dirname "$0")/.."

all="autonomy injection update invariants kill_switch_computer desktop_look"
suites=${*:-$all}
for s in $suites; do
  case " $all " in *" $s "*) ;; *) echo "no such suite: $s (have: $all)" >&2; exit 2 ;; esac
done

# The suites start the server and its model adapter as programs, which
# `cargo test` does not build. ci/on-strix-beta.sh gives each NAME its own
# build directory and copies changed files with fresh times, so the programs
# built here are this checkout's own.
cargo build --workspace --bins

# The desktop-screens invariant drives a real Chromium; Strix Beta has
# Playwright's. Name another with MAGNUS_CHROMIUM.
if [ -z "${MAGNUS_CHROMIUM:-}" ]; then
  for c in "$HOME"/.cache/ms-playwright/chromium-*/chrome-linux64/chrome; do
    [ -x "$c" ] && MAGNUS_CHROMIUM=$c
  done
fi
[ -n "${MAGNUS_CHROMIUM:-}" ] && export MAGNUS_CHROMIUM

failed=
for s in $suites; do
  echo "== acceptance: $s"
  case $s in
    # One test at a time: a machine runs only so many computers, and the
    # invariants that start one fail when tests in parallel take every slot.
    invariants) cmd="cargo test -p magnus-invariants -- --test-threads=1" ;;
    *) cmd="cargo test -p magnus-scratch --test $s" ;;
  esac
  $cmd || failed="$failed $s"
done
[ -z "$failed" ] || { echo "acceptance failed:$failed" >&2; exit 1; }
echo "acceptance passed: $suites"
