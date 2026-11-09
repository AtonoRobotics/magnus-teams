#!/bin/sh
# Installs with apt the package of every command this runner lacks, in one
# install; installs nothing when it has them all. Each argument is
# command=package, e.g. WebKitWebDriver=webkit2gtk-driver.
#
# The installed-app tests need WebKitWebDriver and xclip (build-box-beta
# lacked both: runs 41959422399 and 41961388534). Written inline in the
# workflow, the package list was lost: the step tested `[ -n "" ]` and ran
# `apt-get install -y` with no packages, so it installed nothing (#189).
set -eu
pkgs=""
for pair in "$@"; do
  cmd=${pair%%=*}
  pkg=${pair#*=}
  command -v "$cmd" >/dev/null 2>&1 || pkgs="$pkgs $pkg"
done
[ -n "$pkgs" ] || exit 0
sudo -n apt-get update -qq
# One word per package.
# shellcheck disable=SC2086
sudo -n apt-get install -y $pkgs
