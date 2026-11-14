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

# An outgrown build of the current NAME is emptied; a small one is kept.
r=$t/big; mkdir -p "$r/.running"
name "$r" current "1 hour ago"
echo 900 > "$t/free-big"; echo 41 > "$t/size-big"
STRIX_GC_ROOT=$r STRIX_GC_FREE_GB=$t/free-big STRIX_GC_TARGET_GB=$t/size-big sh "$here/strix-gc.sh" current 2>/dev/null
[ ! -e "$r/targets/current" ] || { echo "FAIL: an outgrown build should be emptied"; fail=1; }
name "$r" current "1 hour ago"; echo 30 > "$t/size-big"
STRIX_GC_ROOT=$r STRIX_GC_FREE_GB=$t/free-big STRIX_GC_TARGET_GB=$t/size-big sh "$here/strix-gc.sh" current 2>/dev/null
kept "$r" current

# Leftover test databases go only when no NAME is running, and postgres stays.
cat > "$t/psql" <<'P'
#!/bin/sh
case "$*" in *"select datname"*) printf 'a_1\nt_2\n' ;; *drop*) echo "$*" >> "$PSQL_LOG" ;; esac
P
chmod +x "$t/psql"
r=$t/db; mkdir -p "$r/.running"; name "$r" current "1 hour ago"; name "$r" other "1 hour ago"
touch "$r/.running/other"
PSQL_LOG=$t/drops STRIX_GC_PG=pg STRIX_GC_PSQL=$t/psql STRIX_GC_ROOT=$r STRIX_GC_FREE_GB=$t/free-big STRIX_GC_TARGET_GB=$t/size-big sh "$here/strix-gc.sh" current 2>/dev/null
[ ! -s "$t/drops" ] || { echo "FAIL: databases dropped while a NAME runs"; fail=1; }
rm "$r/.running/other"
PSQL_LOG=$t/drops STRIX_GC_PG=pg STRIX_GC_PSQL=$t/psql STRIX_GC_ROOT=$r STRIX_GC_FREE_GB=$t/free-big STRIX_GC_TARGET_GB=$t/size-big sh "$here/strix-gc.sh" current 2>/dev/null
[ "$(grep -c 'drop database' "$t/drops" 2>/dev/null)" = 2 ] || { echo "FAIL: leftover databases should be dropped"; fail=1; }
grep -q '"postgres"' "$t/drops" 2>/dev/null && { echo "FAIL: postgres was dropped"; fail=1; }

[ $fail = 0 ] && echo "strix-gc-check: ok"
exit $fail
