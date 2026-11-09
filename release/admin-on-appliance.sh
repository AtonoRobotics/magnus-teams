#!/usr/bin/env bash
# The admin console on the build box's own appliance (DECISIONS.md, "The
# admin console on strix-alpha"): run by CI after each master build is on
# the appliance (release/to-appliance.sh), and safe to run again.
#
#   release/admin-on-appliance.sh CONFIG PROGRAMS_DIR [--config-only]
#
# --config-only stops once [admin] and its certificate are written (the
# test, tests/scratch/tests/admin_on_appliance.rs). MAGNUS_ADMIN_LAN
# ("address/prefix") and MAGNUS_ADMIN_TAILSCALE ("address", or "none") name
# the networks when `ip` should not be asked.
#
# It keeps CONFIG's [admin] section (written the first time, then kept to the
# machine as it is; release/admin-section.py) to what the admin console spec
# leaves to the organisation:
#   - the names it answers to: the machine's Tailscale name when it is on a
#     tailnet (its hostname, which resolves wherever the owner's computers
#     are), its address on the network its default route uses, and its
#     Tailscale address; on port $PORT;
#   - listen: those addresses; never every address or loopback;
#   - allow: those networks only (the LAN's subnet, and Tailscale's
#     100.64.0.0/10);
#   - the certificate: from the organisation's own CA, made here (its key
#     owner-only, beside CONFIG in admin-tls/), naming every name it answers
#     to; and on a tailnet, for its name there, one browsers already trust,
#     which the console has issued through tailscaled and renews (this
#     user may ask tailscaled for it: `tailscale set --operator`; the
#     tailnet needs HTTPS turned on, in its admin console under DNS);
#   - the console's own database role, with a password made here once.
# Then, each time: the role in the database (`magnus migrate`), the
# console's client and service account in the realm and the realm role
# `admin` for the appliance's owners (config/keycloak-client.py, as root,
# which reads the realm administrator's file), and the console as a systemd
# user service beside the appliance, restarted on the programs installed.
# When GOOGLE_CLIENT_SECRET is set, Google's client secret goes into the
# appliance's vault through its owner-only API; it is never printed.
#
# It prints what a person needs to reach the console: its address and the
# CA's certificate (public). It prints no secret.
set -euo pipefail
config=$1 programs=$2 only=${3:-}
port=${MAGNUS_ADMIN_PORT:-7443}
cd "$(dirname "$0")/.."
here=$(dirname "$config")
tls="$here/admin-tls"

# The settings this needs, from the appliance's config.
read -r listen state_dir install < <(python3 -I - "$config" <<'EOF'
import sys, tomllib, os
c = tomllib.load(open(sys.argv[1], "rb"))
print(c["appliance"]["listen"], c["appliance"]["state_dir"], os.path.dirname(c.get("sandbox", {}).get("kernel_image", "")) or "-")
EOF
)
# The names and networks the console answers on, worked out each time:
# the machine's address on the network its default route uses, and its
# Tailscale address and name when it is on a tailnet. The console's
# hostname is its Tailscale name when it has one (it resolves wherever the
# owner's computers are), else the LAN address; the rest are aliases.
lan=${MAGNUS_ADMIN_LAN:-}
if [ -z "$lan" ]; then
  dev=$(ip -o route get 1.1.1.1 | sed -n 's/.* dev \([^ ]*\).*/\1/p')
  lan=$(ip -o -4 addr show dev "$dev" | awk '{print $4}' | head -1)
fi
ts=${MAGNUS_ADMIN_TAILSCALE:-$(ip -o -4 addr show dev tailscale0 2>/dev/null | awk '{print $4}' | head -1 || true)}
[ "$ts" = none ] && ts=""
tsname=${MAGNUS_ADMIN_TSNAME:-}
if [ -z "$tsname" ] && [ -n "$ts" ] && command -v tailscale >/dev/null; then
  tsname=$(tailscale status --json 2>/dev/null | python3 -I -c 'import json, sys; print(json.load(sys.stdin)["Self"]["DNSName"].rstrip("."))' 2>/dev/null || true)
fi
[ "$tsname" = none ] && tsname=""

# The organisation's CA, made once.
mkdir -p "$tls" && chmod 700 "$tls"
if [ ! -f "$tls/ca.pem" ]; then
  openssl req -x509 -new -nodes -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -days 3650 \
    -subj "/O=Magnus appliance $(hostname -s)/CN=Magnus organisation CA" \
    -addext "basicConstraints=critical,CA:true,pathlen:0" -addext "keyUsage=critical,keyCertSign,cRLSign" \
    -keyout "$tls/ca.key" -out "$tls/ca.pem" 2>/dev/null
fi

# The [admin] section this script keeps: written the first time, then kept
# to the names and networks above, with its database password as it was.
# A section someone else wrote is left as it is.
read -r managed sans host < <(python3 -I release/admin-section.py "$config" "$lan" "$ts" "$tsname" "$port" "$tls")
case $managed in
  changed) echo "wrote [admin] to $config: $host, with $sans" ;;
  no) echo "[admin] in $config was not written by this script: left as it is" ;;
esac

# The console's certificate, from the CA, for every name it answers to:
# made again when the names change.
if [ "$managed" != no ] && { [ ! -f "$tls/console.pem" ] || [ "$(cat "$tls/console.sans" 2>/dev/null)" != "$sans" ]; }; then
  openssl req -new -nodes -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -subj "/CN=$host" \
    -keyout "$tls/console.key" -out "$tls/console.csr" 2>/dev/null
  printf 'subjectAltName=%s\nbasicConstraints=critical,CA:false\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\n' "$sans" > "$tls/console.ext"
  openssl x509 -req -in "$tls/console.csr" -CA "$tls/ca.pem" -CAkey "$tls/ca.key" -CAcreateserial \
    -days 825 -extfile "$tls/console.ext" -out "$tls/console.pem" 2>/dev/null
  echo "$sans" > "$tls/console.sans"
  rm -f "$tls/console.csr"
fi
chmod 600 "$tls"/*.key

[ "$only" = --config-only ] && exit 0

# The console, as this user, may have tailscaled issue its certificate.
if [ -n "$tsname" ] && [ "$managed" != no ]; then
  sudo -n tailscale set --operator="$(id -un)" || echo "tailscale set --operator failed: the console keeps the organisation's own certificate"
fi

# The console's database role (and the log's tables, as each release does).
if [ -r "$here/admin-url" ]; then
  "$programs/magnus" migrate --config "$config" --admin-url "$(cat "$here/admin-url")"
fi

# The console's client and service account in the realm, and the realm role
# admin for the appliance's owners, with the console's own key.
mapfile -t names < <(python3 -I -c 'import sys, tomllib; a = tomllib.load(open(sys.argv[1], "rb"))["admin"]; print("\n".join([a["hostname"]] + a.get("aliases", [])))' "$config")
owners=$(python3 -I -c 'import sys, tomllib; print(" ".join(p["id"] for p in tomllib.load(open(sys.argv[1], "rb")).get("people", []) if "owner" in p.get("roles", [])))' "$config")
jwks=$(mktemp)
trap 'rm -f "$jwks"' EXIT
"$programs/magnus-admin" jwk --config "$config" > "$jwks"
assign=()
for o in $owners; do assign+=(--assign-admin "$o"); done
hosts=()
for n in "${names[@]}"; do hosts+=(--admin-hostname "$n:$port"); done
# Keycloak's private admin origin, from the appliance's own identity:
# the client script takes all three as required arguments (#428), so pass
# them explicitly and say which setting is missing when the identity is
# not configured (#312).
read -r kc_url kc_ca < <(python3 -I - "$config" <<'EOF'
import sys, tomllib
try:
    o = tomllib.load(open(sys.argv[1], "rb"))["sign_in"]["oidc"]
    print(o["status_admin_url"], o["status_admin_ca"])
except KeyError:
    print(" ")
EOF
)
kc_admin="$here/identity/admin.json"
[ -n "$kc_url" ] && [ -f "$kc_ca" ] || { echo "no Keycloak admin origin: [sign_in.oidc] status_admin_url/status_admin_ca are missing or the CA is not at $kc_ca (until #429 replaces Keycloak, the live identity is configured by hand)" >&2; exit 1; }
[ -f "$kc_admin" ] || { echo "no Keycloak admin file at $kc_admin (until #429 replaces Keycloak, the live identity is configured by hand)" >&2; exit 1; }
sudo -n python3 -I config/keycloak-client.py --admin-url "$kc_url" --admin-file "$kc_admin" --ca "$kc_ca" --admin-console "$jwks" "${hosts[@]}" "${assign[@]}"

# The console as a user service beside the appliance, on the installed
# programs; restarted so it runs this release.
export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
unit="$HOME/.config/systemd/user/magnus-admin.service"
mkdir -p "$(dirname "$unit")"
cat > "$unit.new" <<EOF
[Unit]
Description=Magnus admin console
After=network-online.target

[Service]
ExecStart=$install/magnus-admin --config $config
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
EOF
mv "$unit.new" "$unit"
systemctl --user daemon-reload
systemctl --user enable magnus-admin.service >/dev/null
systemctl --user restart magnus-admin.service
sleep 3
systemctl --user is-active magnus-admin.service

# NAME.local answers on the network when mDNS runs here.
if systemctl list-unit-files avahi-daemon.service >/dev/null 2>&1; then
  sudo -n systemctl enable --now avahi-daemon.service >/dev/null 2>&1 || true
fi

# Google's and Slack's client secrets, into the appliance's vault through
# its owner-only API, when the repository's secrets are set.
token=
provision() {
  name=$1
  var=$2
  if [ -n "${!var:-}" ]; then
    [ -n "$token" ] || token=$(python3 -I -c 'import sys, tomllib; print(next(p["token"] for p in tomllib.load(open(sys.argv[1], "rb"))["people"] if "owner" in p.get("roles", []) and p.get("token")))' "$config")
    code=$(VAR="$var" python3 -I -c 'import json, os; print(json.dumps({"client_secret": os.environ[os.environ["VAR"]]}))' |
      curl -sS -o /dev/null -w '%{http_code}' -X PUT -H "Authorization: Bearer $token" -H 'Content-Type: application/json' --data-binary @- "http://$listen/v1/registrations/$name")
    echo "$name's client secret into the appliance's vault: HTTP $code"
  else
    echo "$var is not set in the repository: $name's connectors cannot finish signing in yet"
  fi
}
provision google GOOGLE_CLIENT_SECRET
provision slack SLACK_CLIENT_SECRET

# What a person needs to reach it.
echo "The admin console, at any of these on the organisation's network or the tailnet:"
for n in "${names[@]}"; do echo "  https://$n:$port/"; done
if [ -n "$tsname" ] && [ "$managed" != no ]; then
  echo "At https://$tsname:$port/ browsers already trust its certificate once the tailnet issues it (Operations › Network says whether it has, and why not)."
fi
if [ -f "$tls/ca.pem" ]; then
  echo "At its other names, trust this CA once on each computer that opens the console (it is public):"
  cat "$tls/ca.pem"
  echo "SHA-256 fingerprint: $(openssl x509 -in "$tls/ca.pem" -noout -fingerprint -sha256 | cut -d= -f2)"
fi
