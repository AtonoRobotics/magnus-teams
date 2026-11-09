#!/bin/sh
# The appliance's own Postgres (DECISIONS.md, "The appliance's own
# Postgres"): never the dev and test one (`magnus-claude-pg`), whose admin
# password is in git and whose admin every test run is.
#
#   config/install-postgres.sh APPLIANCE_DIR [PORT]
#
# A container `magnus-appliance-pg` from the dev image (dev/postgres.Dockerfile:
# Postgres 16 with pgvector, pinned by digest), on its own volume, listening
# on 127.0.0.1 only (PORT, default 54341), passwords checked with
# scram-sha-256, data checksums on, restarted unless stopped. Its admin
# password is made here, once, and kept only in APPLIANCE_DIR (readable by
# its owner only): `postgres-admin.password`, and `admin-url`, the address
# `magnus migrate` and the master deploy use (release/to-appliance.sh). It
# is never printed. Run again, it only starts the container. It makes the
# database `magnus`; `magnus migrate` makes the rest.
set -eu
dir=$1 port=${2:-54341}
name=magnus-appliance-pg
image=magnus-postgres:16-pgvector
if docker container inspect "$name" >/dev/null 2>&1; then
  exec docker start "$name"
fi
docker image inspect "$image" >/dev/null 2>&1 || docker build -t "$image" -f "$(dirname "$0")/../dev/postgres.Dockerfile" "$(dirname "$0")/../dev"
umask 077
pw="$dir/postgres-admin.password"
[ -s "$pw" ] || openssl rand -hex 32 | tr -d '\n' > "$pw"
docker run -d --name "$name" --restart unless-stopped \
  -p "127.0.0.1:$port:5432" \
  -v magnus-appliance-pgdata:/var/lib/postgresql/data \
  -v "$pw:/run/secrets/postgres-admin:ro" \
  -e POSTGRES_PASSWORD_FILE=/run/secrets/postgres-admin \
  -e POSTGRES_INITDB_ARGS="--data-checksums --auth-host=scram-sha-256" \
  -e POSTGRES_DB=magnus \
  "$image" >/dev/null
printf 'postgres://postgres:%s@127.0.0.1:%s/magnus' "$(cat "$pw")" "$port" > "$dir/admin-url"
end=$(( $(date +%s) + 120 ))
until docker exec "$name" pg_isready -q -U postgres -d magnus; do
  [ "$(date +%s)" -lt "$end" ] || { echo "$name did not answer within two minutes" >&2; exit 1; }
  sleep 2
done
echo "$name is up on 127.0.0.1:$port"
