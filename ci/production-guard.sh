#!/bin/sh
# A Claude Code PreToolUse hook for Bash: on the production server it refuses
# builds and tests, and points at Strix instead
# (decisions/2026-11-12-no-builds-on-the-production-server.md). Builds and the
# test VMs starved the server twice on 2026-11-11; its Wi-Fi stopped
# receiving and the headless machine had to be power-cycled.
#
# Reads the hook's JSON on stdin. Prints a deny decision, or nothing to let
# the command run. Anywhere but the production server it allows everything.
# The hook is one file, so it can be copied where Claude Code runs it.
#
# MAGNUS_PRODUCTION_HOST overrides the production host's name (for testing).
set -eu
production=${MAGNUS_PRODUCTION_HOST:-magnus-server}
[ "$(hostname)" = "$production" ] || exit 0

cmd=$(jq -r '.tool_input.command // empty')
[ -n "$cmd" ] || exit 0

# The command is split, minding quotes, into the simple commands it runs
# (decisions/2026-11-13-review-guest-paths-and-ci.md). Each prints as a line
# "S <text>", its quoted words replaced by _q_ so a commit message is never
# read as a command. A command whose program is ci/on-strix-beta.sh prints
# nothing: its arguments run on Strix. Text a shell runs as a command here is
# split the same way and checked too: the argument of sh/bash -c, a
# here-document fed to a shell, and "$(...)" or a backquote inside double
# quotes. Nesting deeper than a few levels prints "X", refused.
split_commands='
function isshell(w) { sub(/.*\//, "", w); return w ~ /^(sh|bash|dash|zsh|ksh|ash)$/ }
function shell_before(words, j,    m) { for (m = 0; m < j; m++) if (isshell(words[m])) return 1; return 0 }
# Prints one simple command; returns whether it is a shell reading its stdin.
function finish(words, nw, stext, depth,    k, w, prog, j) {
  if (nw == 0) return 0
  for (k = 0; k < nw; k++) {
    w = words[k]
    if (w ~ /^[A-Za-z_][A-Za-z0-9_]*=/ || w ~ /^-/ || w ~ /^[0-9.]+[smhd]?$/ || w == "!" || w == "{") continue
    if (w ~ /^(timeout|nice|env|exec|time|command|nohup|sudo)$/) continue
    break
  }
  prog = words[k]
  if (isshell(prog) && k + 1 < nw && words[k + 1] !~ /^-/) prog = words[k + 1]
  if (prog ~ /(^|\/)ci\/on-strix-beta\.sh$/) return 0
  print "S " stext
  for (j = 0; j < nw - 1; j++)
    if (words[j] ~ /^-[A-Za-z]*c$/ && shell_before(words, j)) scan(words[j + 1], depth + 1)
  return isshell(words[k]) && !(k + 1 < nw && words[k + 1] !~ /^-/)
}
function heredocs(s, p, depth, hd, hdd, hsh, nhd,    k, L, e, line, body) {
  L = length(s)
  for (k = 0; k < nhd; k++) {
    body = ""
    while (p <= L) {
      e = index(substr(s, p), "\n")
      if (e == 0) { line = substr(s, p); p = L + 1 } else { line = substr(s, p, e - 1); p += e }
      if (hdd[k]) sub(/^\t+/, "", line)
      if (line == hd[k]) break
      body = body line "\n"
    }
    if (hsh[k]) scan(body, depth + 1)
  }
  return p
}
function scan(s, depth,    i, L, c, d, j, q, t, word, has, stext, words, nw, hd, hdd, hds, hsh, nhd, segno, sh) {
  if (depth > 6) { print "X"; return }
  L = length(s); i = 1; word = ""; has = 0; stext = ""; nw = 0; nhd = 0; segno = 0
  while (i <= L) {
    c = substr(s, i, 1)
    if (c == "\\") {
      d = substr(s, i + 1, 1)
      if (d != "\n") { word = word d; has = 1; stext = stext c d }
      i += 2; continue
    }
    if (c == "\047") {
      j = index(substr(s, i + 1), "\047")
      if (j == 0) j = L - i + 1
      word = word substr(s, i + 1, j - 1); has = 1; stext = stext "_q_"
      i += j + 1; continue
    }
    if (c == "\"") {
      q = ""; i++
      while (i <= L && (d = substr(s, i, 1)) != "\"") {
        if (d == "\\" && i < L) { i++; d = substr(s, i, 1) }
        q = q d; i++
      }
      i++
      if (q ~ /\$\(|`/) scan(q, depth + 1)
      word = word q; has = 1; stext = stext "_q_"; continue
    }
    if (c == "<" && substr(s, i, 2) == "<<" && substr(s, i + 2, 1) != "<") {
      if (has) { words[nw++] = word; word = ""; has = 0 }
      i += 2; hdd[nhd] = 0
      if (substr(s, i, 1) == "-") { hdd[nhd] = 1; i++ }
      while (substr(s, i, 1) ~ /[ \t]/) i++
      t = ""
      while (i <= L && (d = substr(s, i, 1)) !~ /[ \t\n;&|<>()]/) { if (d != "\047" && d != "\"" && d != "\\") t = t d; i++ }
      hd[nhd] = t; hds[nhd] = segno; hsh[nhd] = 0; nhd++
      stext = stext " _h_ "; continue
    }
    if (c ~ /[;&|\n()`]/ && !(c == "&" && substr(s, i - 1, 1) ~ /[<>]/)) {
      if (has) { words[nw++] = word; word = ""; has = 0 }
      sh = finish(words, nw, stext, depth)
      for (j = 0; j < nhd; j++) if (hds[j] == segno) hsh[j] = sh
      segno++; nw = 0; stext = ""; split("", words)
      if (c == "\n" && nhd > 0) { i = heredocs(s, i + 1, depth, hd, hdd, hsh, nhd); nhd = 0; continue }
      i++; continue
    }
    if (c == " " || c == "\t") {
      if (has) { words[nw++] = word; word = ""; has = 0 }
      stext = stext c; i++; continue
    }
    word = word c; has = 1; stext = stext c; i++
  }
  if (has) { words[nw++] = word }
  finish(words, nw, stext, depth)
}
BEGIN { scan(ENVIRON["GUARD_CMD"], 0) }
'
pieces=$(GUARD_CMD=$cmd awk "$split_commands") || pieces=X
commands=$(printf '%s\n' "$pieces" | sed -n 's/^S //p')

# A cargo command that compiles, where a simple command starts, or after
# timeout, nice, env or a VAR=value prefix.
start='(^|\b(timeout|nice|env|exec|time)\b.*|[A-Za-z_][A-Za-z0-9_]*=[^ ]*)[[:space:]]*'
cargo='([^[:space:]]*/)?cargo([[:space:]]+\+[^[:space:]]+)?[[:space:]]+(build|b|test|t|check|c|clippy|nextest|bench|run|r|llvm-cov|tauri[[:space:]]+(build|dev))\b'
others='(^|[[:space:]])([^[:space:]]*/)?(ci/ratchet\.sh[[:space:]]+clippy|ci/acceptance\.sh|cargo-nextest)\b'
if printf '%s\n' "$pieces" | grep -q '^X' || printf '%s\n' "$commands" | grep -Eq "$start$cargo" || printf '%s\n' "$commands" | grep -Eq "$others"; then
  reason="This is the production server ($production). Builds and tests never run here; they starved it twice on 2026-11-11 and it had to be power-cycled. Run it on Strix Beta instead: ci/on-strix-beta.sh <name> <command>, for example ci/on-strix-beta.sh my-change cargo test --workspace --lib --bins. cargo fmt, ci/ratchet.sh size and git are fine here."
  jq -n --arg r "$reason" '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $r}}'
fi
exit 0
