#!/bin/sh
# Whether the runner has the disk a run needs, checked as each CI job starts
# (.github/workflows/*.yml). The build box filled during master's run on
# #199's merge: with 1.3 GB free, Chromium crashed in four tests and the
# update test could not unpack its release (#203). Short of disk, the run
# fails here, at once, saying why.
#
#   release/ci-disk.sh [DIR...]
#
# Each DIR (default: the checkout, which holds target/, and the temporary
# directory) must have MAGNUS_CI_FREE_GB free (default 25: a run keeps
# target/, about 21 GB on the build box on 2026-11-05, and a dependency or
# toolchain change writes all of it again; the installers, the update test's
# two AppImages and their extractions, and the screenshots add about 1 GB).
set -eu
need=${MAGNUS_CI_FREE_GB:-25}
[ $# -gt 0 ] || set -- . "${TMPDIR:-/tmp}"
short=0
for d in "$@"; do
  # Available kilobytes, and the filesystem's mount point.
  set -- $(df -Pk "$d" | awk 'NR == 2 { print $4, $6 }')
  free=$(( $1 / 1048576 ))
  if [ "$free" -lt "$need" ]; then
    echo "::error::$free GB free on $2 ($d); a run needs $need GB. Clear what earlier runs left (release/ci-clean.sh, docker system df) before running again." >&2
    short=1
  else
    echo "$free GB free on $2 ($d); a run needs $need GB"
  fi
done
exit "$short"
