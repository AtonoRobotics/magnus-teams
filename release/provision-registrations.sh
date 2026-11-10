#!/bin/bash
# The server's own client with each catalog provider, from the repository's
# secrets, into the server's vault (connectors spec rev 46: "no person types
# it, and nothing shows it again"; decisions/2026-11-10-provisioned-client-ids.md).
# Each provider's client is a pair, <PREFIX>_CLIENT_ID and
# <PREFIX>_CLIENT_SECRET: the ID is provisioned beside the secret, so the
# server signs in with the client the secret belongs to, whatever ID the
# catalog (catalog/registrations.toml) has. A public client takes no secret.
# Until a confidential provider's secret is there, no one can connect its
# apps on that server.
#
# It goes through the kernel's control socket, which only the server's own
# user can open, so it needs no person's token. Values pass through the
# environment and stdin only. Nothing prints a value: per provider, only
# whether the repository's client ID matches the catalog's, and the answer.
#
#   release/provision-registrations.sh CONFIG
set -euo pipefail
config=$1
catalog=$(dirname "$0")/../catalog/registrations.toml
state=$(python3 -I -c 'import sys, tomllib; print(tomllib.load(open(sys.argv[1], "rb"))["appliance"]["state_dir"])' "$config")
case $state in /*) ;; *) state=$(dirname "$config")/$state ;; esac
socket=$state/control.sock
[ -S "$socket" ] || { echo "no control socket at $socket: is the server running?" >&2; exit 1; }

# The catalog's field for a provider ("" when it has none).
field() { python3 -I -c 'import sys, tomllib; print(tomllib.load(open(sys.argv[1], "rb")).get(sys.argv[2], {}).get(sys.argv[3], ""))' "$catalog" "$1" "$2"; }

# "match", "differs" or "not set": the variable named $1 against the
# catalog's value $2, compared without printing either; "set" when the
# catalog names none (a client only ever provisioned, like Asana's).
compare() {
  VAR="$1" WANT="$2" python3 -I -c '
import os
v = os.environ.get(os.environ["VAR"], "").strip()
w = os.environ["WANT"]
print("not set" if not v else "set" if not w else "match" if v == w else "differs")'
}

failed=0
# provider, and the prefix of its pair of repository secrets.
provision() {
  provider=$1 prefix=$2
  kind=$(field "$provider" kind)
  [ -n "$kind" ] || { echo "$provider: no registration in the catalog"; failed=1; return 0; }
  want=$(field "$provider" client_id)
  if [ -n "$want" ]; then
    echo "$provider: ${prefix}_CLIENT_ID $(compare "${prefix}_CLIENT_ID" "$want") the catalog's client ID"
  else
    echo "$provider: ${prefix}_CLIENT_ID $(compare "${prefix}_CLIENT_ID" "") (the catalog names no client ID: this one is the only one)"
  fi
  body=$(ID_VAR="${prefix}_CLIENT_ID" SECRET_VAR="${prefix}_CLIENT_SECRET" KIND="$kind" python3 -I -c '
import json, os
e = os.environ
b = {"actor": "release"}
i = e.get(e["ID_VAR"], "").strip()
s = e.get(e["SECRET_VAR"], "").strip()
if i:
    b["client_id"] = i
if s and e["KIND"] != "public":
    b["client_secret"] = s
print(json.dumps(b) if len(b) > 1 else "")')
  if [ -z "$body" ]; then
    echo "$provider: neither ${prefix}_CLIENT_ID nor ${prefix}_CLIENT_SECRET is set in the repository: nothing provisioned"
    return 0
  fi
  code=$(printf '%s' "$body" |
    curl -sS -o /dev/null -w '%{http_code}' --unix-socket "$socket" -X PUT -H 'Content-Type: application/json' \
      --data-binary @- "http://server/v1/admin/registrations/$provider") || code=000
  id_var=${prefix}_CLIENT_ID secret_var=${prefix}_CLIENT_SECRET
  what="client ID $([ -n "${!id_var:-}" ] && echo provisioned || echo "from the catalog")"
  [ "$kind" = public ] || what="$what, client secret $([ -n "${!secret_var:-}" ] && echo provisioned || echo "not set")"
  echo "$provider ($kind): $what: HTTP $code"
  [ "${code:0:1}" = 2 ] || failed=1
}

provision google GOOGLE_DESKTOP
provision microsoft MICROSOFT
provision github GH_OAUTH
provision vercel VERCEL
provision slack SLACK
provision sentry SENTRY
provision asana ASANA

# The repository's other Google clients and Microsoft's tenant, compared
# only, so a mismatch is seen: the catalog's Google client is a desktop one.
for prefix in GOOGLE GOOGLE_WEB; do
  echo "google: ${prefix}_CLIENT_ID $(compare "${prefix}_CLIENT_ID" "$(field google client_id)") the catalog's client ID (not provisioned: the catalog's client is a desktop one)"
done
echo "microsoft: MICROSOFT_TENANT_ID $(compare MICROSOFT_TENANT_ID "$(field microsoft tenant)") the catalog's tenant"

exit $failed
