#!/bin/sh
# Stamps the desktop app's own page with its version: every stylesheet and
# script the page loads is asked for as NAME?v=VERSION. The app's webview
# keeps what it loaded under a URL and reuses it after an update, so with
# the same plain names an updated app went on showing the old interface.
# The app finds a bundled file by its path, so the query changes nothing
# else (decisions/2026-11-18-desktop-ui-urls-carry-the-version.md).
#   release/stamp-ui.sh VERSION [PAGE]   (PAGE: desktop/ui/index.html)
set -eu
version="$1"
page="${2:-desktop/ui/index.html}"
case "$version" in *[!0-9A-Za-z.+-]*|"") echo "not a version: $version" >&2; exit 2 ;; esac
sed -i -E "s/\.(css|js)\"/.\1?v=$version\"/g" "$page"
if grep -Eq '(href|src)="[^"?]+\.(css|js)"' "$page"; then echo "$page still loads a file by its plain name" >&2; exit 1; fi
