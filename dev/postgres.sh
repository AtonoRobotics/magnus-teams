#!/bin/sh
# The dev and test Postgres (`magnus-claude-pg`): `dev/postgres.Dockerfile`
# (Postgres 16 with pgvector, by digest) on 127.0.0.1, with the address and
# admin password the tests use by default (tests/invariants/src/pg.rs, read
# from there). It holds only test databases: each test makes its own and
# drops it.
#
#   dev/postgres.sh          start it, making it first if it is not there
#   dev/postgres.sh --again  remove it and its data, and make it again, empty
#
# Its settings, all here: 1000 connections (tests run side by side), dynamic
# shared memory in mmap (at 1000 connections it outgrew Docker's 64 MB
# /dev/shm), and no durability (fsync, synchronous_commit, full_page_writes
# off: Postgres's documentation, "Non-Durable Settings", for data that need
# not survive a crash). Without durability a power cut can leave its data
# damaged; dev/services.sh then makes it again by itself (DECISIONS.md, "The
# test Postgres runs without durability"). Never the appliance's own
# Postgres (config/install-postgres.sh), which keeps every setting durable.
set -eu
here=$(dirname "$0")
name=magnus-claude-pg
volume=magnus-claude-pgdata
image=magnus-postgres:16-pgvector
set -- "${1:-}" $(sed -nE 's#.*"postgres://postgres:([^@]+)@127\.0\.0\.1:([0-9]+)/postgres".*#\1 \2#p' "$here/../tests/invariants/src/pg.rs")
again=$1 pw=${2:?no default test Postgres address in tests/invariants/src/pg.rs} port=${3:?}
if [ "$again" = --again ]; then
  docker rm -f -v "$name" >/dev/null 2>&1 || true
  docker volume rm -f "$volume" >/dev/null
fi
if docker container inspect "$name" >/dev/null 2>&1; then
  exec docker start "$name" >/dev/null
fi
docker image inspect "$image" >/dev/null 2>&1 || docker build -q -t "$image" -f "$here/postgres.Dockerfile" "$here" >/dev/null
docker run -d --name "$name" --restart unless-stopped \
  -p "127.0.0.1:$port:5432" \
  -v "$volume:/var/lib/postgresql/data" \
  -e POSTGRES_PASSWORD="$pw" \
  "$image" \
  -c max_connections=1000 -c dynamic_shared_memory_type=mmap \
  -c fsync=off -c synchronous_commit=off -c full_page_writes=off >/dev/null
