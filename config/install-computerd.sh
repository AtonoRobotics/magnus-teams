#!/bin/sh
# Installs magnus-computerd, the root service that starts computer VMs. Run
# with sudo on any machine that has computers, and again to update it:
#
#   sudo sh install-computerd.sh <magnus-computerd> <vmlinux> <rootfs.ext4> [user Magnus runs as]
#
# Everything root reads afterwards is copied here into root-owned places, so
# nothing the appliance's own user can write is trusted. It also adds the
# user to the magnus group, which reaches the service's socket; Magnus picks
# that up when it next starts. Running computers are left alone.
set -eu
[ "$(id -u)" = 0 ] || { echo "run this with sudo" >&2; exit 1; }
[ $# -ge 3 ] || { echo "usage: sudo sh $0 <magnus-computerd> <vmlinux> <rootfs.ext4> [user]" >&2; exit 1; }
binary=$1 kernel=$2 rootfs=$3 user=${4:-${SUDO_USER:-}}
[ -n "$user" ] && id "$user" >/dev/null 2>&1 || { echo "name the user Magnus runs as" >&2; exit 1; }
here=$(dirname "$0")
for tool in /usr/local/bin/jailer /usr/local/bin/firecracker; do
  [ -x "$tool" ] || { echo "$tool is missing" >&2; exit 1; }
done

getent group magnus >/dev/null || groupadd --system magnus
usermod -aG magnus "$user"
gid=$(getent group magnus | cut -d: -f3)

state=/var/lib/magnus-computerd
install -d -m 0755 -o root -g root "$state" /usr/local/lib/magnus /etc/magnus
install -d -m 0700 -o root -g root "$state/computers" "$state/jail"
install -d -m 0755 -o root -g root "$state/images"
install -m 0755 -o root -g root "$binary" /usr/local/lib/magnus/magnus-computerd.new
mv -f /usr/local/lib/magnus/magnus-computerd.new /usr/local/lib/magnus/magnus-computerd
# A new inode: running VMs keep the image they booted from.
for pair in "$kernel:vmlinux" "$rootfs:rootfs.ext4"; do
  from=${pair%%:*} name=${pair##*:}
  cp -L --sparse=always "$from" "$state/images/$name.new"
  chown root:root "$state/images/$name.new"
  chmod 0644 "$state/images/$name.new"
  mv -f "$state/images/$name.new" "$state/images/$name"
done

# Overlays and snapshots are valid only on the base image they were made on.
sha256sum "$state/images/rootfs.ext4" | cut -d' ' -f1 > "$state/images/base.id"
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
systemctl daemon-reload
systemctl enable magnus-computerd >/dev/null
systemctl restart magnus-computerd
echo "magnus-computerd is running; add [computer] socket = \"/run/magnus-computerd.sock\" to magnus.toml"
