#!/bin/sh
# Ratchets for rules the code already breaks: new code must pass, and the
# existing count may only fall. The baselines are committed beside this file.
#   ci/ratchet.sh size     no .rs file over 800 lines unless it is in ci/oversize.txt and has not grown
#   ci/ratchet.sh clippy   clippy warnings (unwrap and expect among them) do not exceed ci/clippy-baseline
set -eu
cd "$(dirname "$0")/.."

case "${1:-}" in
size)
  limit=800
  now=$(mktemp)
  git ls-files '*.rs' | xargs wc -l | awk -v lim="$limit" '$2 != "total" && $1 > lim { print $2, $1 }' | sort > "$now"
  awk 'NR == FNR { base[$1] = $2; next }
       !($1 in base) { printf "new file over the limit: %s (%d lines)\n", $1, $2; bad = 1 }
       ($1 in base) && $2 > base[$1] { printf "%s grew: %d lines, baseline %d\n", $1, $2, base[$1]; bad = 1 }
       END { exit bad }' ci/oversize.txt "$now"
  ;;
clippy)
  out=$(mktemp)
  cargo clippy --workspace --all-targets --message-format=json > "$out" 2>/dev/null || true
  errors=$(jq -r 'select(.reason == "compiler-message" and .message.level == "error") | .message.rendered' "$out" | wc -l)
  if [ "$errors" -gt 0 ]; then
    jq -r 'select(.reason == "compiler-message" and .message.level == "error") | .message.rendered' "$out" | head -40
    echo "clippy or the build reports errors" >&2
    exit 1
  fi
  count=$(jq -r 'select(.reason == "compiler-message" and .message.level == "warning" and (.message.spans | length) > 0) | "\(.message.code.code // "none") \(.message.spans[0].file_name):\(.message.spans[0].line_start)"' "$out" | sort -u | wc -l)
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
