#!/bin/sh
# Prints the SSH destination for Strix Beta and the known-host name to check
# it against, on one line: "DESTINATION HOST_KEY_ALIAS"
# (decisions/2026-11-28-strix-beta-over-the-wired-link.md).
#
# magnus-server and Strix Beta share a 2.5 Gb/s wired network (10.0.1.0/24,
# static addresses on both). Their Wi-Fi addresses carried a release's
# 8.6 GB sandbox image at about 4 MB/s, long enough for the release runner
# to lose its server; the wired link carries it at about 270 MB/s. So the
# wired address is used when it answers, and the Wi-Fi LAN address otherwise
# (a machine not on the wired network, or the cable out).
#
# STRIX_BETA sets the destination outright, as before, and
# STRIX_BETA_HOST_KEY_ALIAS the known-host name (by default the destination's
# host). STRIX_BETA_WIRED and STRIX_BETA_LAN change the two candidates.
set -eu
WIRED_PROBE_TIMEOUT_SECONDS=3
wired=${STRIX_BETA_WIRED:-samuel@10.0.1.11}
lan=${STRIX_BETA_LAN:-samuel@192.168.0.51}
# The wired address is checked against Strix Beta's key as "strix-beta";
# the LAN address keeps the name each caller used before.
wired_alias=strix-beta
lan_alias=${STRIX_BETA_LAN_HOST_KEY_ALIAS:-${lan#*@}}

if [ -n "${STRIX_BETA:-}" ]; then
  echo "$STRIX_BETA ${STRIX_BETA_HOST_KEY_ALIAS:-${STRIX_BETA#*@}}"
  exit 0
fi
if ssh -o BatchMode=yes -o ConnectTimeout=$WIRED_PROBE_TIMEOUT_SECONDS \
     -o HostKeyAlias=$wired_alias "$wired" true </dev/null >/dev/null 2>&1; then
  echo "$wired ${STRIX_BETA_HOST_KEY_ALIAS:-$wired_alias}"
else
  echo "$lan ${STRIX_BETA_HOST_KEY_ALIAS:-$lan_alias}"
fi
