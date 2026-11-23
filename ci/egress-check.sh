#!/bin/sh
# Checks that an HTTP client is built only where the allowlist below says
# (decisions/2026-11-23-one-egress-client.md). An address someone else chose
# (an added app's, every address its sign-in names, a generated connector's,
# a page the agent fetches) is reached only through
# crates/magnus/src/egress_client.rs, which checks and pins every address and
# every redirect. Every other file here reaches a fixed address the operator
# configured, or is not the server, and says which.
#
# Test code is not checked: files named *_tests.rs, tests.rs or testing.rs,
# and a file's `#[cfg(test)] mod ... {` to its end.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

# path: why it may build a client.
allowed='
crates/magnus/src/egress_client.rs: the one client for addresses someone else chose, and `configured` for the relay and the search service the config names
crates/magnus/src/oidc.rs: the identity provider the server admin configured
crates/magnus/src/devices.rs: the Cloudflare API the config names, for device certificates
crates/magnus/src/knowledge.rs: the embeddings server the config names
crates/magnus/src/payments/link.rs: Link, at the API and login addresses the config names
crates/magnus/src/push_subs/mod.rs: Microsoft Graph and Google, at fixed addresses, for catalog apps only
crates/magnus/src/ui/images.rs: the image provider the config names
crates/adapter-anthropic/src/lib.rs: a model adapter, reaching the model provider the operator configured
crates/adapter-openai/src/lib.rs: a model adapter, reaching the model provider the operator configured
crates/local/src/lib.rs: the local program on a computer, reaching its own server
desktop/src-tauri/src/bridge.rs: the desktop app, reaching its own server
desktop/src-tauri/src/pin.rs: the desktop app, reaching its own server
'

found=$(find crates desktop/src-tauri/src -name '*.rs' -not -path '*/target/*' -not -path '*/tests/*' \
  -not -name '*_tests.rs' -not -name 'tests.rs' -not -name 'testing.rs' | sort | while read -r f; do
  awk -v f="$f" '
    /^#\[cfg\(test\)\]/ { cfg = NR; next }
    cfg == NR - 1 && /^(pub(\([a-z]+\))? )?mod [A-Za-z_0-9]+ *\{/ { exit }
    /Client::builder\(|Client::new\(|ClientBuilder::new\(|Client::default\(|reqwest::get\(|reqwest::blocking/ { print f ":" NR ": " $0 }
  ' "$f"
done)

fail=0
printf '%s\n' "$found" | while IFS= read -r line; do
  [ -n "$line" ] || continue
  path=${line%%:*}
  if ! printf '%s\n' "$allowed" | grep -q "^$path: "; then
    echo "FAIL: $line" >&2
    echo "  an HTTP client is built outside the allowlist in ci/egress-check.sh: reach an address someone else chose through crate::egress_client, or add this file to the allowlist with the fixed address it reaches" >&2
    exit 1
  fi
done || fail=1

# Every allowlisted file still builds one: a stale entry is removed.
printf '%s\n' "$allowed" | while IFS= read -r entry; do
  [ -n "$entry" ] || continue
  path=${entry%%: *}
  printf '%s\n' "$found" | grep -q "^$path:" || { echo "FAIL: $path is allowlisted in ci/egress-check.sh but builds no client; remove it" >&2; exit 1; }
done || fail=1

[ "$fail" = 0 ] && echo "egress check passed"
exit "$fail"
