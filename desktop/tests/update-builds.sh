#!/bin/sh
# The update test's two AppImages (installed-app test
# `the_installed_app_updates_itself_from_the_appliance_while_a_task_waits`,
# and the channels test): VERSION and the next, with a key made
# for the run, so nothing it signs could install anywhere.
#
#   desktop/tests/update-builds.sh VERSION OUT_DIR
#
# Writes OUT_DIR/{old,new}.AppImage and their .sig, and OUT_DIR/new.version.
# CI runs it beside the other tests (.github/workflows/desktop.yml).
set -eu
v=$1 t=$2
cd "$(dirname "$0")/../src-tauri"
# This script also runs outside the installer workflow step; its two real
# AppImages need the same verified runtime as the published installer.
export LDAI_RUNTIME_FILE
LDAI_RUNTIME_FILE=$(../../release/ensure-appimage-runtime.sh)
mkdir -p "$t"
cargo tauri signer generate --ci -p "" -f -w "$t/test.key" >/dev/null
export TAURI_SIGNING_PRIVATE_KEY="$t/test.key" TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
next="${v%.*}.$(( ${v##*.} + 1 ))"
for build in "old $v" "new $next"; do
  set -- $build
  cargo tauri build --bundles appimage --config "{\"version\":\"$2\",\"plugins\":{\"updater\":{\"pubkey\":\"$(cat "$t/test.key.pub")\"}}}"
  cp "target/release/bundle/appimage/Magnus_$2_amd64.AppImage" "$t/$1.AppImage"
  cp "target/release/bundle/appimage/Magnus_$2_amd64.AppImage.sig" "$t/$1.AppImage.sig"
done
echo "$next" > "$t/new.version"
