#!/bin/sh
# Checks ci/production-guard.sh: on the production host it refuses builds and
# tests that are not sent to Strix, and it lets everything else through.
# Pretends this host is the production one (MAGNUS_PRODUCTION_HOST).
set -eu
guard=$(dirname "$0")/production-guard.sh
here=$(hostname)
fail=0
check() {
  want=$1 host=$2 cmd=$3
  out=$(jq -n --arg c "$cmd" '{tool_name: "Bash", tool_input: {command: $c}}' | MAGNUS_PRODUCTION_HOST=$host sh "$guard")
  if [ -n "$out" ]; then got=deny; else got=allow; fi
  if [ "$got" != "$want" ]; then echo "FAIL: want $want, got $got: $cmd" >&2; fail=1; fi
}
for c in \
  "cargo test --workspace --lib --bins" \
  "CARGO_INCREMENTAL=0 /home/samuel/.cargo/bin/cargo test -p magnus" \
  "timeout 1800 cargo test -p magnus-invariants --test inv4_taint" \
  "cd x && cargo build --release" \
  "echo hi | cargo clippy" \
  "cargo +nightly check" \
  "(cd desktop/src-tauri && cargo tauri build --bundles deb)" \
  "ci/ratchet.sh clippy" \
  "sh ci/acceptance.sh"; do
  check deny "$here" "$c"
  check allow "not-$here" "$c"
done
for c in \
  "ci/on-strix-beta.sh my-change cargo test --workspace" \
  "sh ci/on-strix-beta.sh a sh -c 'cargo build'" \
  "cargo fmt --all --check" \
  "ci/ratchet.sh size" \
  "cargo tree" \
  "git commit -m 'run cargo test on strix'" \
  "ls; grep cargo build.rs"; do
  check allow "$here" "$c"
done
[ $fail = 0 ] && echo "production guard: ok"
exit $fail
