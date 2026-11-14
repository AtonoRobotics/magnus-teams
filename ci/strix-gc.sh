#!/bin/sh
# Garbage-collect the build directories that ci/on-strix-beta.sh leaves on
# Strix Beta, so its disk never fills (decisions/2026-11-13-strix-beta-gc.md).
# ci/on-strix-beta.sh runs this on Strix Beta before and after each run.
#
#   sh ci/strix-gc.sh CURRENT_NAME
#
# Every NAME has a checkout (ROOT/NAME) and a build directory
# (ROOT/targets/NAME). A NAME is removed, both directories, when it is idle
# and either:
#   - it has not been used for KEEP_IDLE_HOURS, or
#   - the disk has less than MIN_FREE_GB free; then the least recently used
#     idle NAMEs go first, until there is room again.
# The current NAME is never removed. A NAME is running, so not idle, while
# its marker (ROOT/.running/NAME) is younger than MAX_RUN_HOURS: a marker
# older than that is left by a run whose connection died.
#
# Cargo never deletes an old build, so the current NAME's build directory
# grows with every change; past MAX_TARGET_GB it is emptied and rebuilt.
#
# The tests create a database for each server and account they start and do
# not always drop it. With STRIX_GC_PG set (the test Postgres), when no NAME
# is running every database but postgres is dropped.
#
# STRIX_GC_ROOT, STRIX_GC_FREE_GB, STRIX_GC_TARGET_GB and STRIX_GC_PSQL stand
# in for the real root, the measured free space, the measured build size and
# psql in ci/strix-gc-check.sh.
set -eu
KEEP_IDLE_HOURS=24
MIN_FREE_GB=200
MAX_RUN_HOURS=12
MAX_TARGET_GB=40
current=${1:?usage: strix-gc.sh CURRENT_NAME}
root=${STRIX_GC_ROOT:-$HOME/magnus-teams-ci}
mkdir -p "$root/.used" "$root/.running" "$root/targets"

now=$(date +%s)
# When NAME was last used, in seconds since the epoch: its stamp, or for a
# NAME from before stamps, its newest directory.
last_used() {
  for f in "$root/.used/$1" "$root/targets/$1" "$root/$1"; do
    [ -e "$f" ] && { stat -c %Y "$f"; return; }
  done
  echo 0
}
running() {
  [ -e "$root/.running/$1" ] && [ $((now - $(stat -c %Y "$root/.running/$1"))) -lt $((MAX_RUN_HOURS * 3600)) ]
}
free_gb() {
  if [ -n "${STRIX_GC_FREE_GB:-}" ]; then cat "$STRIX_GC_FREE_GB"; else df -P -BG "$root" | awk 'NR==2 { sub("G", "", $4); print $4 }'; fi
}
remove() {
  echo "strix-gc: removing $1 (last used $(( (now - $(last_used "$1")) / 3600 ))h ago)" >&2
  rm -rf "${root:?}/targets/$1" "${root:?}/$1" "$root/$1.bundle" "$root/.used/$1" "$root/.running/$1"
  # The check's stand-in for the disk: a removed NAME frees what it held.
  if [ -n "${STRIX_GC_FREE_GB:-}" ]; then echo $(($(cat "$STRIX_GC_FREE_GB") + 100)) > "$STRIX_GC_FREE_GB"; fi
}

# Every NAME, idle ones only, least recently used first.
idle=$(
  { ls -1 "$root/targets"; ls -1 "$root"; } 2>/dev/null | sort -u | while read -r n; do
    case $n in targets | sandbox-out | *.bundle | "$current") continue ;; esac
    [ -d "$root/$n" ] || [ -d "$root/targets/$n" ] || continue
    running "$n" && continue
    echo "$(last_used "$n") $n"
  done | sort -n | cut -d' ' -f2
)
for n in $idle; do
  if [ $((now - $(last_used "$n"))) -ge $((KEEP_IDLE_HOURS * 3600)) ] || [ "$(free_gb)" -lt "$MIN_FREE_GB" ]; then
    remove "$n"
  fi
done

# The current NAME's build, when it has outgrown MAX_TARGET_GB.
target_gb() {
  if [ -n "${STRIX_GC_TARGET_GB:-}" ]; then cat "$STRIX_GC_TARGET_GB"; else du -s -BG "$root/targets/$current" 2>/dev/null | awk '{ sub("G", "", $1); print $1 }'; fi
}
if [ -d "$root/targets/$current" ] && [ "$(target_gb)" -gt "$MAX_TARGET_GB" ]; then
  echo "strix-gc: emptying the build of $current (over ${MAX_TARGET_GB} GB)" >&2
  rm -rf "${root:?}/targets/$current"
fi

# Leftover test databases, once nothing is running.
if [ -n "${STRIX_GC_PG:-}" ]; then
  busy=0
  for m in "$root/.running"/*; do
    [ -e "$m" ] && running "$(basename "$m")" && busy=1
  done
  if [ $busy = 0 ]; then
    psql=${STRIX_GC_PSQL:-psql}
    $psql "$STRIX_GC_PG" -Atqc "select datname from pg_database where not datistemplate and datname <> 'postgres'" | while read -r d; do
      [ -n "$d" ] && $psql "$STRIX_GC_PG" -qc "drop database \"$d\" with (force)" >/dev/null && echo "strix-gc: dropped test database $d" >&2
    done
  fi
fi
