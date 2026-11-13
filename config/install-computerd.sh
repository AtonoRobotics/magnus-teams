#!/bin/sh
# Installs magnus-computerd, the root service that starts computer VMs. Run
# with sudo on any machine that has computers, and again to update it:
#
#   sudo sh install-computerd.sh <magnus-computerd> <vmlinux> <computer.ext4> [user Magnus runs as] [release directory]
#
# <computer.ext4> is the computer's base image, the minimal one a release
# carries for computers (decisions/2026-11-13-minimal-computer-image.md),
# not the per-task sandbox's rootfs.ext4. It is installed as the service's
# images/rootfs.ext4, the name its base image has always had here.
#
# Everything root reads afterwards is copied here into root-owned places, so
# nothing the appliance's own user can write is trusted. It also adds the
# user to the magnus group, which reaches the service's socket; Magnus picks
# that up when it next starts. Running computers are left alone.
#
# Given the release directory Magnus updates itself from (its `[updates]
# dir`, then `appliance`: by default <state_dir>/updates/appliance), it also
# sets up the service's own updates: magnus-computerd-update runs whenever a
# release is published there (and hourly), and installs a newer one only
# after checking its signature with the release key root keeps in
# /etc/magnus/release.pub (`magnus-computerd self-update`). Without it, an
# earlier setting is kept.
#
# A VERSION file beside <magnus-computerd> (a release has one) is installed
# beside the program: it is what the service tells Magnus it is.
set -eu
[ "$(id -u)" = 0 ] || { echo "run this with sudo" >&2; exit 1; }
[ $# -ge 3 ] || { echo "usage: sudo sh $0 <magnus-computerd> <vmlinux> <computer.ext4> [user] [release directory]" >&2; exit 1; }
binary=$1 kernel=$2 rootfs=$3 user=${4:-${SUDO_USER:-}} releases=${5:-}
[ -n "$user" ] && id "$user" >/dev/null 2>&1 || { echo "name the user Magnus runs as" >&2; exit 1; }
here=$(dirname "$0")
for tool in /usr/local/bin/jailer /usr/local/bin/firecracker; do
  [ -x "$tool" ] || { echo "$tool is missing" >&2; exit 1; }
done
if [ -n "$releases" ]; then
  releases=$(cd "$releases" && pwd -P) || { echo "no release directory $5" >&2; exit 1; }
  links_from=$(cd "$(dirname "$binary")" && pwd -P)
  case "$releases$links_from$user" in *'"'* | *'\'*) echo "a path or user with a quote or backslash" >&2; exit 1 ;; esac
fi

getent group magnus >/dev/null || groupadd --system magnus
usermod -aG magnus "$user"
gid=$(getent group magnus | cut -d: -f3)

state=/var/lib/magnus-computerd
lib=/usr/local/lib/magnus
install -d -m 0755 -o root -g root "$state" "$lib" /etc/magnus
install -d -m 0700 -o root -g root "$state/computers" "$state/jail"
install -d -m 0755 -o root -g root "$state/images"
install -m 0755 -o root -g root "$binary" "$lib/magnus-computerd.new"
mv -f "$lib/magnus-computerd.new" "$lib/magnus-computerd"
if [ -f "$(dirname "$binary")/VERSION" ]; then
  install -m 0644 -o root -g root "$(dirname "$binary")/VERSION" "$lib/VERSION.new"
  mv -f "$lib/VERSION.new" "$lib/VERSION"
else
  rm -f "$lib/VERSION"
fi
# A new inode: running VMs keep the image they booted from. An update whose
# image is the one installed passes the installed file itself.
for pair in "$kernel:vmlinux" "$rootfs:rootfs.ext4"; do
  from=${pair%%:*} name=${pair##*:}
  if [ "$from" -ef "$state/images/$name" ]; then continue; fi
  cp -L --sparse=always "$from" "$state/images/$name.new"
  chown root:root "$state/images/$name.new"
  chmod 0644 "$state/images/$name.new"
  mv -f "$state/images/$name.new" "$state/images/$name"
  if [ "$name" = rootfs.ext4 ]; then rm -f "$state/images/base.id"; fi
done

# Overlays and snapshots are valid only on the base image they were made on.
if [ ! -s "$state/images/base.id" ]; then
  sha256sum "$state/images/rootfs.ext4" | cut -d' ' -f1 > "$state/images/base.id"
fi
chmod 0644 "$state/images/base.id"

if [ ! -f /etc/magnus/computerd.toml ]; then
  install -m 0644 -o root -g root /dev/null /etc/magnus/computerd.toml
  cat > /etc/magnus/computerd.toml <<EOF
state_dir = "$state"
socket = "/run/magnus-computerd.sock"
socket_gid = $gid
jailer = "/usr/local/bin/jailer"
firecracker = "/usr/local/bin/firecracker"
first_uid = 200000
max_computers = 256
max_live = 8

[vm]
vcpus = 2
memory_mib = 4096
cpu_percent = 200
pids_max = 256
volume_gib = 20
overlay_gib = 10
EOF
fi

install -m 0644 -o root -g root "$here/magnus-computerd.service" /etc/systemd/system/magnus-computerd.service

# The service's own updates. The key is root's copy, made once from the
# program root installed; a release never replaces it.
if [ -n "$releases" ]; then
  install -m 0644 -o root -g root /dev/null /etc/magnus/computerd-update.toml.new
  cat > /etc/magnus/computerd-update.toml.new <<EOF
releases = "$releases"
links_from = "$links_from"
user = "$user"
EOF
  mv -f /etc/magnus/computerd-update.toml.new /etc/magnus/computerd-update.toml
fi
if [ -f /etc/magnus/computerd-update.toml ]; then
  if [ ! -s /etc/magnus/release.pub ]; then
    "$lib/magnus-computerd" release-key > /etc/magnus/release.pub.new
    chmod 0644 /etc/magnus/release.pub.new
    mv -f /etc/magnus/release.pub.new /etc/magnus/release.pub
  fi
  watched=$(sed -n 's/^releases = "\(.*\)"$/\1/p' /etc/magnus/computerd-update.toml)
  cat > /etc/systemd/system/magnus-computerd-update.service <<EOF
[Unit]
Description=Magnus computerd: install a newer signed release
After=magnus-computerd.service

[Service]
Type=oneshot
ExecStart=$lib/magnus-computerd self-update
EOF
  cat > /etc/systemd/system/magnus-computerd-update.path <<EOF
[Unit]
Description=Magnus computerd: watch for a new release

[Path]
PathChanged=$watched/latest.json
Unit=magnus-computerd-update.service

[Install]
WantedBy=multi-user.target
EOF
  cat > /etc/systemd/system/magnus-computerd-update.timer <<EOF
[Unit]
Description=Magnus computerd: look for a new release hourly

[Timer]
OnBootSec=5min
OnUnitActiveSec=1h
Unit=magnus-computerd-update.service

[Install]
WantedBy=timers.target
EOF
  chmod 0644 /etc/systemd/system/magnus-computerd-update.service /etc/systemd/system/magnus-computerd-update.path /etc/systemd/system/magnus-computerd-update.timer
fi
systemctl daemon-reload
systemctl enable magnus-computerd >/dev/null
systemctl restart magnus-computerd
if [ -f /etc/magnus/computerd-update.toml ]; then
  systemctl enable --now magnus-computerd-update.path magnus-computerd-update.timer >/dev/null
  echo "magnus-computerd updates itself from signed releases in $watched"
else
  echo "magnus-computerd does not update itself: run this again naming the release directory to set that up"
fi
echo "magnus-computerd is running; add [computer] socket = \"/run/magnus-computerd.sock\" to magnus.toml"
