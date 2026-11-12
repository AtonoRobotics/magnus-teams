#!/bin/sh
# A Claude Code PreToolUse hook for Bash: on the production server it refuses
# builds and tests, and points at Strix instead
# (decisions/2026-11-12-no-builds-on-the-production-server.md). Builds and the
# test VMs starved the server twice on 2026-11-11; its Wi-Fi stopped
# receiving and the headless machine had to be power-cycled.
#
# Reads the hook's JSON on stdin. Prints a deny decision, or nothing to let
# the command run. Anywhere but the production server it allows everything.
#
# MAGNUS_PRODUCTION_HOST overrides the production host's name (for testing).
set -eu
production=${MAGNUS_PRODUCTION_HOST:-magnus-server}
[ "$(hostname)" = "$production" ] || exit 0

cmd=$(jq -r '.tool_input.command // empty')
[ -n "$cmd" ] || exit 0

# Sent to Strix: the build and the tests run there, not here.
case $cmd in *ci/on-strix-beta.sh*) exit 0 ;; esac

# A cargo command that compiles, where a command starts: at the start of a
# line, after ; & | ( or a backquote, or after timeout, nice, env or a
# VAR=value prefix. Text inside quotes, such as a commit message, is not a
# command start and is left alone.
start='(^|[;&|(`]|\b(timeout|nice|env|exec|time)\b[^;&|]*|[A-Za-z_][A-Za-z0-9_]*=[^ ]*)[[:space:]]*'
cargo='([^[:space:]]*/)?cargo([[:space:]]+\+[^[:space:]]+)?[[:space:]]+(build|b|test|t|check|c|clippy|nextest|bench|run|r|llvm-cov|tauri[[:space:]]+(build|dev))\b'
others='(^|[;&|(`[:space:]])([^[:space:]]*/)?(ci/ratchet\.sh[[:space:]]+clippy|ci/acceptance\.sh|cargo-nextest)\b'
if printf '%s\n' "$cmd" | grep -Eq "$start$cargo" || printf '%s\n' "$cmd" | grep -Eq "$others"; then
  reason="This is the production server ($production). Builds and tests never run here; they starved it twice on 2026-11-11 and it had to be power-cycled. Run it on Strix Beta instead: ci/on-strix-beta.sh <name> <command>, for example ci/on-strix-beta.sh my-change cargo test --workspace --lib --bins. cargo fmt, ci/ratchet.sh size and git are fine here."
  jq -n --arg r "$reason" '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $r}}'
fi
exit 0
