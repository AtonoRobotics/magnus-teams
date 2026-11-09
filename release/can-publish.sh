#!/bin/sh
# Whether this run publishes.
#
#   release/can-publish.sh CONFIG MUST
#
# Prints 1 when this runner has the release key (~/.magnus-release/update.key,
# or MAGNUS_RELEASE_KEY) and the appliance at CONFIG, whose updates directory
# the owner's app reads; else 0. With MUST=1 (master, a tag), a runner that
# lacks either fails here, before anything is built. The key alone used to
# decide: strix-beta got the key and has no appliance, so master run
# 42097633195 published the test build into a directory no app reads and
# then failed installing on an appliance that is not there (#204).
set -eu
config=$1
must=$2
key=${MAGNUS_RELEASE_KEY:-$HOME/.magnus-release/update.key}
if [ -f "$key" ] && [ -f "$config" ]; then
  echo 1
  exit 0
fi
if [ "$must" = 1 ]; then
  [ -f "$key" ] || echo "this run publishes, and this runner has no release key ($key)" >&2
  [ -f "$config" ] || echo "this run publishes, and this runner has no appliance ($config)" >&2
  exit 1
fi
echo 0
