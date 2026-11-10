#!/bin/sh
# Acceptance: the heavy suites a release candidate runs before it ships
# (AGENTS.md, "How work lands"). Never a gate on a push; fast.yml is that.
# A release that fails one does not ship. Run it on Strix Beta:
#
#   ci/on-strix-beta.sh acceptance ci/acceptance.sh            every suite
#   ci/on-strix-beta.sh acceptance ci/acceptance.sh autonomy   one suite
#
# Suites:
#   autonomy    the autonomy scenarios (spec rev 133 and 140, "Autonomy is
#               tested like safety"), tests/scratch/tests/autonomy.rs
#   injection   prompt injection, tests/scratch/tests/injection.rs
#   update      signed updates, tests/scratch/tests/update.rs
#   invariants  the eight invariants in full, tests/invariants
set -eu
cd "$(dirname "$0")/.."

all="autonomy injection update invariants"
suites=${*:-$all}
for s in $suites; do
  case " $all " in *" $s "*) ;; *) echo "no such suite: $s (have: $all)" >&2; exit 2 ;; esac
done

# The suites start the server and its model adapter as programs, which
# `cargo test` does not build. ci/on-strix-beta.sh gives each NAME its own
# build directory and copies changed files with fresh times, so the programs
# built here are this checkout's own.
cargo build --workspace --bins

failed=
for s in $suites; do
  echo "== acceptance: $s"
  case $s in
    invariants) cmd="cargo test -p magnus-invariants" ;;
    *) cmd="cargo test -p magnus-scratch --test $s" ;;
  esac
  $cmd || failed="$failed $s"
done
[ -z "$failed" ] || { echo "acceptance failed:$failed" >&2; exit 1; }
echo "acceptance passed: $suites"
