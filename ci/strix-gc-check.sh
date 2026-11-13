#!/bin/sh
# Checks ci/strix-gc.sh against a fake ~/magnus-teams-ci: which NAMEs it
# removes and which it keeps (decisions/2026-11-13-strix-beta-gc.md).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
fail=0

# A NAME with a checkout and a build directory, last used AGE ago.
name() { mkdir -p "$1/$2/src" "$1/targets/$2/debug" "$1/.used"; touch -d "$3" "$1/$2" "$1/targets/$2"; touch -d "$3" "$1/.used/$2"; }
gone() { [ ! -e "$1/$2" ] && [ ! -e "$1/targets/$2" ] || { echo "FAIL: $2 should be removed"; fail=1; }; }
kept() { [ -d "$1/$2" ] || [ -d "$1/targets/$2" ] || { echo "FAIL: $2 should be kept"; fail=1; }; }

# Plenty of room: only NAMEs idle for over a day go.
r=$t/idle; mkdir -p "$r/.running" "$r/sandbox-out"
name "$r" old "48 hours ago"
name "$r" recent "1 hour ago"
name "$r" current "72 hours ago"
name "$r" busy "48 hours ago"; touch -d "1 hour ago" "$r/.running/busy"
name "$r" died "48 hours ago"; touch -d "13 hours ago" "$r/.running/died"
mkdir -p "$r/targets/legacy"; touch -d "48 hours ago" "$r/targets/legacy"
echo 900 > "$t/free-idle"
STRIX_GC_ROOT=$r STRIX_GC_FREE_GB=$t/free-idle sh "$here/strix-gc.sh" current 2>/dev/null
gone "$r" old; gone "$r" died; gone "$r" legacy
kept "$r" recent; kept "$r" current; kept "$r" busy
[ -d "$r/sandbox-out" ] || { echo "FAIL: the shared guest image was removed"; fail=1; }

# Low on room: idle NAMEs go least recently used first, until there is room
# (each removal frees 100 GB in this stand-in), and never the current or a
# running one.
r=$t/full; mkdir -p "$r/.running"
name "$r" a "3 hours ago"
name "$r" b "2 hours ago"
name "$r" c "1 hour ago"
name "$r" current "4 hours ago"
name "$r" busy "5 hours ago"; touch "$r/.running/busy"
echo 50 > "$t/free-full"
STRIX_GC_ROOT=$r STRIX_GC_FREE_GB=$t/free-full sh "$here/strix-gc.sh" current 2>/dev/null
gone "$r" a; gone "$r" b
kept "$r" c; kept "$r" current; kept "$r" busy

[ $fail = 0 ] && echo "strix-gc-check: ok"
exit $fail
