#!/bin/bash
# Alpha Vector's client secrets with Google and Slack, from the repository's
# secrets (GOOGLE_CLIENT_SECRET, SLACK_CLIENT_SECRET), into the server's
# vault (connectors spec rev 46: "no person types it, and nothing shows it
# again"). Until a provider's secret is there, no one can connect its apps
# on that server. It goes through the kernel's control socket, which only
# the server's own user can open, so it needs no person's token. A secret
# that is not set is skipped and said; no secret is ever printed.
#
#   release/provision-registrations.sh CONFIG
set -euo pipefail
config=$1
state=$(python3 -I -c 'import sys, tomllib; print(tomllib.load(open(sys.argv[1], "rb"))["appliance"]["state_dir"])' "$config")
case $state in /*) ;; *) state=$(dirname "$config")/$state ;; esac
socket=$state/control.sock
[ -S "$socket" ] || { echo "no control socket at $socket: is the server running?" >&2; exit 1; }

# provider, the variable its secret is in, and one connector that signs in
# with it (the secret serves every connector of that provider).
provision() {
  provider=$1 var=$2 connector=$3
  if [ -z "${!var:-}" ]; then
    echo "$var is not set in the repository: $provider's connectors cannot be connected yet"
    return 0
  fi
  code=$(VAR="$var" python3 -I -c 'import json, os; print(json.dumps({"actor": "release", "value": os.environ[os.environ["VAR"]]}))' |
    curl -sS -o /dev/null -w '%{http_code}' --unix-socket "$socket" -X PUT -H 'Content-Type: application/json' \
      --data-binary @- "http://server/v1/admin/connectors/$connector/credential")
  echo "$provider's client secret into the server's vault: HTTP $code"
  [ "${code:0:1}" = 2 ]
}
provision google GOOGLE_CLIENT_SECRET google-gmail
provision slack SLACK_CLIENT_SECRET slack
