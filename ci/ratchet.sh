#!/bin/sh
# Ratchets for rules the code already breaks: new code must pass, and the
# existing count may only fall. The baselines are committed beside this file.
#   ci/ratchet.sh size     no .rs file over 800 lines unless it is in ci/oversize.txt at its current length
#   ci/ratchet.sh clippy   clippy warnings (unwrap and expect among them) do not exceed ci/clippy-baseline
set -eu
cd "$(dirname "$0")/.."

case "${1:-}" in
size)
  limit=800
  now=$(mktemp)
  # Tracked files and new ones not yet added (but not ignored ones).
  { git ls-files '*.rs'; git ls-files --others --exclude-standard '*.rs'; } | sort -u | xargs wc -l | awk -v lim="$limit" '$2 != "total" && $1 > lim { print $2, $1 }' | sort > "$now"
  # A baseline is the file's current length: one above it would let the file
  # grow back, so a file that shrinks has its baseline lowered with it.
  awk -v lim="$limit" 'NR == FNR { base[$1] = $2; next }
       { seen[$1] = 1 }
       !($1 in base) { printf "new file over the limit: %s (%d lines)\n", $1, $2; bad = 1 }
       ($1 in base) && $2 > base[$1] { printf "%s grew: %d lines, baseline %d\n", $1, $2, base[$1]; bad = 1 }
       ($1 in base) && $2 < base[$1] { printf "%s shrank to %d lines: lower its baseline in ci/oversize.txt from %d\n", $1, $2, base[$1]; bad = 1 }
       END { for (f in base) if (!(f in seen)) { printf "%s is at most %d lines now: take it out of ci/oversize.txt\n", f, lim; bad = 1 }
             exit bad }' ci/oversize.txt "$now"
  ;;
clippy)
  out=$(mktemp)
  # Fails closed: a clippy that did not run, did not finish its build, or
  # wrote no readable message stream is a failure, never zero warnings.
  status=0
  cargo clippy --workspace --all-targets --message-format=json > "$out" 2>/dev/null || status=$?
  if ! jq -e -s 'any(.[]; .reason == "build-finished" and .success == true)' "$out" > /dev/null 2>&1; then
    jq -r 'select(.reason == "compiler-message" and .message.level == "error") | .message.rendered' "$out" 2>/dev/null | head -40 || true
    echo "clippy did not finish a successful build (exit $status), or its output is not cargo's JSON" >&2
    exit 1
  fi
  [ "$status" -eq 0 ] || { echo "clippy exited $status" >&2; exit 1; }
  errors=$(jq -r 'select(.reason == "compiler-message" and .message.level == "error") | .message.rendered' "$out" | wc -l)
  if [ "$errors" -gt 0 ]; then
    jq -r 'select(.reason == "compiler-message" and .message.level == "error") | .message.rendered' "$out" | head -40
    echo "clippy or the build reports errors" >&2
    exit 1
  fi
  # One per warning and place; a warning with no place counts once per text.
  # rustc's "N warnings emitted" summary is not a warning of its own.
  count=$(jq -r 'select(.reason == "compiler-message" and .message.level == "warning")
    | select(.message.message | test("^[0-9]+ warnings? emitted") | not)
    | if (.message.spans | length) > 0
      then "\(.message.code.code // "none") \(.message.spans[0].file_name):\(.message.spans[0].line_start)"
      else "\(.message.code.code // "none") nowhere: \(.message.message)" end' "$out" | sort -u | wc -l)
  if [ ! -f ci/clippy-baseline ]; then
    echo "no baseline: clippy warnings = $count" >&2
    exit 1
  fi
  base=$(cat ci/clippy-baseline)
  echo "clippy warnings: $count (baseline $base)"
  [ "$count" -le "$base" ] || { echo "clippy warnings rose above the baseline" >&2; exit 1; }
  ;;
*)
  echo "usage: ci/ratchet.sh size|clippy" >&2
  exit 2
  ;;
esac
