#!/bin/sh
# Installs the Linux libraries the desktop app (desktop/src-tauri) compiles
# against, for the fast tier's desktop job on a hosted runner. The release's
# own build machine and Strix Beta have them already. One list, here: Tauri's
# WebKitGTK webview (GTK, libsoup and the rest come with it), libxdo for its
# menus, and D-Bus for the keyring's secret service.
set -eu
sudo apt-get update -qq
sudo apt-get install -y -qq --no-install-recommends \
  libwebkit2gtk-4.1-dev \
  libxdo-dev \
  libdbus-1-dev \
  pkg-config
