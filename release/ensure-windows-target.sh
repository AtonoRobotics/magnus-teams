#!/bin/sh
# The Windows installer is cross-built. A magnus-linux runner that has
# rustup but not the gnu Windows target stops release-checks at Installers,
# before any test (build-box-beta, master runs 41958146895 and 41955875225:
# "Target x86_64-pc-windows-gnu is not installed").
set -eu
target=x86_64-pc-windows-gnu
if rustup target list --installed | grep -qx "$target"; then
  exit 0
fi
rustup target add "$target"
