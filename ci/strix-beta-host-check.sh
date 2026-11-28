#!/bin/sh
# Checks ci/strix-beta-host.sh and that the scripts reaching Strix Beta use it
# (decisions/2026-11-28-strix-beta-over-the-wired-link.md). Reaches no real
# host: the wired candidate is an address that never answers.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
helper=$root/ci/strix-beta-host.sh
fail=0
bad() { echo "FAIL: $*" >&2; fail=1; }
# TEST-NET-1 (RFC 5737): routed nowhere, so the probe fails fast.
dead=nobody@192.0.2.1

got=$(STRIX_BETA_WIRED=$dead sh "$helper")
[ "$got" = "samuel@192.168.0.51 192.168.0.51" ] || bad "an unreachable wired link does not fall back to the LAN address: $got"

got=$(STRIX_BETA_WIRED=$dead STRIX_BETA_LAN_HOST_KEY_ALIAS=strix-beta sh "$helper")
[ "$got" = "samuel@192.168.0.51 strix-beta" ] || bad "the LAN fallback ignores its known-host name: $got"

got=$(STRIX_BETA=me@example STRIX_BETA_WIRED=$dead sh "$helper")
[ "$got" = "me@example example" ] || bad "STRIX_BETA does not override the destination: $got"

grep -q 'WIRED_PROBE_TIMEOUT_SECONDS=' "$helper" || bad "the wired probe has no named timeout"
grep -q 'samuel@10.0.1.11' "$helper" || bad "the wired address is not the default first choice"

for f in release/fetch-build.sh ci/on-strix-beta.sh; do
  grep -q 'strix-beta-host.sh' "$root/$f" || bad "$f does not choose Strix Beta's address with ci/strix-beta-host.sh"
  ! grep -q 'STRIX_BETA:-' "$root/$f" || bad "$f still hard-codes a default Strix Beta address"
done

[ "$fail" = 0 ] && echo "strix-beta host check passed"
exit "$fail"
