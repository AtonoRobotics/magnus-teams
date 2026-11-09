#!/bin/sh
# The dev and test Keycloak (`magnus-claude-kc`), a real Keycloak in dev mode
# on 127.0.0.1:54380, admin `admin` / `magnus_dev`. Tests make a realm of
# their own on it and delete it when they end (tests/scratch/src/keycloak.rs),
# as they make a database of their own on `magnus-claude-pg`.
# The image is pinned by digest: 26.8.1, the one already on this machine.
set -eu
IMAGE=quay.io/keycloak/keycloak@sha256:a78d6b79572168cb25886b80a8b5b8d4ed4390743e3eecb810538683e209dd6b
if docker container inspect magnus-claude-kc >/dev/null 2>&1; then
  exec docker start magnus-claude-kc
fi
exec docker run -d --name magnus-claude-kc --restart unless-stopped \
  -p 127.0.0.1:54380:8080 \
  -e KC_BOOTSTRAP_ADMIN_USERNAME=admin -e KC_BOOTSTRAP_ADMIN_PASSWORD=magnus_dev \
  "$IMAGE" start-dev
