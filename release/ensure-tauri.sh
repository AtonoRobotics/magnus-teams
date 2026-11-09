#!/bin/sh
# Makes `cargo tauri` available and prints the directory to keep on PATH
# (one line, for $GITHUB_PATH). cargo-tauri is an installed binary, not a
# rustup component. A runner that has rustup and the magnus-linux label but
# never had the binary stops release-checks at "no such command: tauri"
# before any test (build-box-beta, 2026-11-05, master run 41942081672).
set -eu
bindir=${CARGO_HOME:-$HOME/.cargo}/bin
mkdir -p "$bindir"
export PATH="$bindir:$PATH"
if ! cargo tauri --version >/dev/null 2>&1; then
  cargo install tauri-cli --version "^2" --locked
fi
cargo tauri --version >/dev/null
printf '%s\n' "$bindir"
