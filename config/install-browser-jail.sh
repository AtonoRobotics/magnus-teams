#!/bin/sh
# Run once with sudo on any machine that runs the browser, naming the user
# Magnus runs as (default: the user who ran sudo). It puts the jail's own copy
# of bubblewrap at /usr/local/lib/magnus/bwrap (the `[browser] bwrap` default),
# usable only by the magnus group. Where AppArmor restricts unprivileged user
# namespaces, as Ubuntu 24.04 does, it allows them for that copy only, so the
# system's own bwrap and every other program stay restricted. Safe to run again.
# Magnus picks up the group when it next starts; a shell needs a new login.
set -eu
[ "$(id -u)" = 0 ] || { echo "run this with sudo" >&2; exit 1; }
source=$(command -v bwrap) || { echo "install bubblewrap first (apt-get install bubblewrap)" >&2; exit 1; }
user=${1:-${SUDO_USER:-}}
[ -n "$user" ] && id "$user" >/dev/null 2>&1 || { echo "usage: sudo sh $0 <user Magnus runs as>" >&2; exit 1; }
dir=/usr/local/lib/magnus
getent group magnus >/dev/null || groupadd --system magnus
usermod -aG magnus "$user"
install -d -m 0755 "$dir"
install -m 0750 -o root -g magnus "$source" "$dir/bwrap"
if [ "$(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns 2>/dev/null || echo 0)" = 1 ]; then
  cat > /etc/apparmor.d/magnus-bwrap <<EOF
abi <abi/4.0>,
include <tunables/global>

profile magnus-bwrap $dir/bwrap flags=(unconfined) {
  userns,
}
EOF
  apparmor_parser -r /etc/apparmor.d/magnus-bwrap
fi
# Run as the Magnus user, since root would pass where that user cannot.
setpriv --reuid "$user" --regid "$(id -g "$user")" --init-groups \
  "$dir/bwrap" --unshare-all --ro-bind /usr /usr --symlink usr/lib /lib --symlink usr/lib64 /lib64 --symlink usr/bin /bin --proc /proc --dev /dev -- true
echo "the browser jail works for $user: $dir/bwrap"
