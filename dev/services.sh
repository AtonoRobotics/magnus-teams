#!/bin/sh
# Start the build box's dev and test services, `magnus-claude-pg`
# (dev/postgres.sh) and `magnus-claude-kc` (dev/keycloak.sh), and wait until
# both answer. CI runs this before any test: the runner can take a job before
# the containers are back after a reboot (2026-10-31, an injection run failed
# so). Both start in seconds here (Keycloak in 5-7 s); two minutes is the
# limit.
#
# The test Postgres runs without durability (dev/postgres.sh), so a power cut
# can leave its data damaged. If it does not answer a query in two minutes,
# it is made again, empty, and waited for once more: it holds only test
# databases, so nothing is lost, and CI never waits on someone to rebuild it.
set -eu
here=$(dirname "$0")
"$here/postgres.sh"
"$here/keycloak.sh" >/dev/null

# Wait until $1 succeeds, for two minutes.
up() {
  end=$(( $(date +%s) + 120 ))
  until eval "$1" >/dev/null 2>&1; do
    [ "$(date +%s)" -lt "$end" ] || return 1
    sleep 2
  done
}
pg='docker exec magnus-claude-pg psql -U postgres -Atc "SELECT 1"'
if ! up "$pg"; then
  echo "::warning::the test Postgres did not answer in two minutes; making it again, empty"
  docker logs --tail 20 magnus-claude-pg >&2 || true
  "$here/postgres.sh" --again
  up "$pg" || { echo "the test Postgres, made again, did not answer within two minutes" >&2; exit 1; }
fi
up 'curl -fsS -o /dev/null http://127.0.0.1:54380/realms/master' || { echo "Keycloak did not answer within two minutes" >&2; exit 1; }
echo "Postgres and Keycloak are up"
