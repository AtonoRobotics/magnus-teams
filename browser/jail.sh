#!/bin/sh
# Starts Chromium in its jail. Playwright runs this as the browser program, so
# "$@" is Chromium's own arguments, and fds 3 and 4 are its control pipe.
#
# Chromium reads hostile pages, on the machine that holds the vault and the
# log. In here it has its own sandbox on, sees no host files beyond /usr and
# its own install, and has no network: its only way out is relay.js, to the
# egress proxy on a Unix socket in the work directory. If this cannot start,
# nothing runs: there is no unconfined fallback.
set -eu
: "${MAGNUS_BWRAP:?}" "${MAGNUS_CHROME:?}" "${MAGNUS_NODE:?}" "${MAGNUS_WORK:?}" "${MAGNUS_RELAY:?}"
: "${MAGNUS_PROXY_SOCK:?}" "${MAGNUS_PROXY_PORT:?}"
chrome_dir=$(dirname "$MAGNUS_CHROME")
exec "$MAGNUS_BWRAP" \
  --unshare-all --new-session --die-with-parent --cap-drop ALL --clearenv \
  --ro-bind /usr /usr \
  --symlink usr/bin /bin --symlink usr/sbin /sbin --symlink usr/lib /lib --symlink usr/lib64 /lib64 \
  --ro-bind-try /etc/fonts /etc/fonts \
  --ro-bind-try /etc/ld.so.cache /etc/ld.so.cache \
  --ro-bind-try /var/cache/fontconfig /var/cache/fontconfig \
  --ro-bind-try /sys/devices/system/cpu /sys/devices/system/cpu \
  --ro-bind "$chrome_dir" "$chrome_dir" \
  --ro-bind "$MAGNUS_NODE" "$MAGNUS_NODE" \
  --ro-bind "$MAGNUS_RELAY" "$MAGNUS_RELAY" \
  --tmpfs /tmp \
  --bind "$MAGNUS_WORK" "$MAGNUS_WORK" \
  --proc /proc --dev /dev \
  --setenv HOME "$MAGNUS_WORK" --setenv TMPDIR "$MAGNUS_WORK" \
  --setenv PATH /usr/bin \
  --setenv FONTCONFIG_PATH /etc/fonts \
  -- "$MAGNUS_NODE" "$MAGNUS_RELAY" "$MAGNUS_PROXY_PORT" "$MAGNUS_PROXY_SOCK" "$MAGNUS_CHROME" "$@"
