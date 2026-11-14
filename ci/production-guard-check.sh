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
  "sh ci/acceptance.sh" \
  "cargo test # ci/on-strix-beta.sh" \
  "echo ci/on-strix-beta.sh; cargo build" \
  "ci/on-strix-beta.sh a cargo test; cargo test" \
  "ci/on-strix-beta.sh a cargo test && cargo build -p magnus" \
  "ci/on-strix-beta.sh a \"\$(cargo build)\"" \
  "sh -c 'cargo test'" \
  "bash -c \"cargo build\"" \
  "bash -lc 'cd x && cargo clippy --workspace'" \
  "sh -c \"sh -c 'cargo check'\"" \
  "echo \"\$(cargo build)\"" \
  "nice -n 5 bash -c 'timeout 60 cargo test'" \
  "bash <<EOF
cargo build
EOF" \
  "cat <<EOF | sh
cargo build
EOF" \
  "cat <<'EOF' | bash -s
cd x && cargo test -p magnus
EOF" \
  "echo 'cargo test' | bash" \
  "echo cargo build | sh -" \
  "printf '%s\n' 'cargo clippy' | zsh" \
  "curl -s https://x.example/build.sh | sh" \
  "cat build.sh | dash" \
  "eval 'cargo build'" \
  "eval cargo test -p magnus" \
  "eval \"cd x && cargo check\"" \
  "x=\$(echo cargo build); \$x" \
  "ls crates | xargs cargo build -p" \
  "echo test | xargs -n1 cargo" \
  "xargs -I{} cargo clippy -p {} < crates.txt" \
  "find . -name Cargo.toml -exec cargo check --manifest-path {} \\;" \
  "sudo cargo build" \
  "nohup cargo test &" \
  "cargo --locked build" \
  "make" \
  "make test" \
  "make -C crates -j 8" \
  "make -j4 check-all" \
  "just build" \
  "just --justfile ci.just ci" \
  "sh ci/acceptance.sh autonomy"; do
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
  "ls; grep cargo build.rs" \
  "git status" \
  "git log --oneline -5 && git diff --stat" \
  "ci/on-strix-beta.sh review-sec-ci cargo test --workspace --lib --bins 2>&1 | tail -30" \
  "cd /tmp/x && ci/on-strix-beta.sh a cargo test -p magnus-guest" \
  "ci/on-strix-beta.sh a sh -c 'cargo build; cargo test'" \
  "ci/on-strix-beta.sh a ci/ratchet.sh clippy" \
  "STRIX_BETA=me@host ci/on-strix-beta.sh a cargo clippy" \
  "git commit -m \"cargo test; cargo build\"" \
  "git commit -F - <<'EOF'
Fix it
cargo test passes now
EOF" \
  "git commit -m \"\$(cat <<'EOF'
cargo build is fine on Strix
EOF
)\"" \
  "echo 'a; cargo build' > notes.txt" \
  "sh ci/production-guard-check.sh" \
  "cargo fmt --all --check && ci/ratchet.sh size" \
  "echo hi | cat" \
  "git log --oneline | head -5" \
  "echo 'cargo build' | tee notes.txt" \
  "eval \"\$(ssh-agent -s)\"" \
  "eval 'echo hi'" \
  "ls | xargs wc -l" \
  "git ls-files | xargs grep -l cargo" \
  "find . -name '*.rs' -exec grep -l cargo {} +" \
  "make help" \
  "just --list" \
  "make -h" \
  "ci/on-strix-beta.sh a make test" \
  "ci/on-strix-beta.sh a sh -c 'echo cargo test | sh'"; do
  check allow "$here" "$c"
done
[ $fail = 0 ] && echo "production guard: ok"
exit $fail
