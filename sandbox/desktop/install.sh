#!/bin/sh
# Puts the Magnus desktop into the image (sandbox/Dockerfile runs it from
# the directory holding sandbox/desktop's files). Spec rev 145, "The desktop".
set -eu
src=$(cd "$(dirname "$0")" && pwd)
share=/usr/share/magnus/desktop

python3 -I "$src/make_theme.py" /tmp/magnus-theme
cp -r /tmp/magnus-theme/themes/Magnus-Light /tmp/magnus-theme/themes/Magnus-Dark /usr/share/themes/
mkdir -p "$share" /usr/share/backgrounds/magnus /usr/local/lib/magnus/desktop
cp /tmp/magnus-theme/magnus/light.vars /tmp/magnus-theme/magnus/dark.vars "$src/terminalrc" "$share/"
# Terminal's settings before the desktop first sets the look: dark.
mkdir -p /etc/xdg/xfce4/terminal
(. "$share/dark.vars"; cat "$src/terminalrc"; printf 'ColorForeground=%s\nColorBackground=%s\nColorCursor=%s\nColorPalette=%s\n' "$TERM_FG" "$TERM_BG" "$TERM_CURSOR" "$TERM_PALETTE") >/etc/xdg/xfce4/terminal/terminalrc
# The Magnus menu's star, as the icon named `magnus`.
mkdir -p /usr/share/icons/hicolor/scalable/apps
cp /tmp/magnus-theme/magnus/star.svg /usr/share/icons/hicolor/scalable/apps/magnus.svg
gtk-update-icon-cache -q -f /usr/share/icons/hicolor || true
# The wallpaper at twice the screen, as the artboard's README asks.
for m in light dark; do
  cp "/tmp/magnus-theme/magnus/wallpaper-$m.svg" "/usr/share/backgrounds/magnus/magnus-$m.svg"
  rsvg-convert -w 2560 -h 1600 -o "/usr/share/backgrounds/magnus/magnus-$m.png" "/tmp/magnus-theme/magnus/wallpaper-$m.svg"
done
rm -rf /tmp/magnus-theme

# Papirus, recolored to the theme: folders take the app's blue accent, and
# the symbolic icons the light and dark text colors.
papirus=/usr/share/icons/Papirus
rm -rf /usr/share/icons/ePapirus /usr/share/icons/ePapirus-Dark
find "$papirus"/*/places -type f -name '*.svg' -exec sed -i -e 's/#5294e2/#3557d9/Ig' -e 's/#4877b1/#2c43ad/Ig' {} +
find /usr/share/icons/Papirus-Dark -type f -name '*.svg' -exec sed -i -e 's/#dfdfdf/#d1d1d6/Ig' {} +
find /usr/share/icons/Papirus-Light -type f -name '*.svg' -exec sed -i -e 's/#444444/#3f3f44/Ig' {} +
for t in Papirus Papirus-Dark Papirus-Light; do gtk-update-icon-cache -q -f "/usr/share/icons/$t" || true; done

xfconf=/etc/xdg/xfce4/xfconf/xfce-perchannel-xml
mkdir -p "$xfconf"
cp "$src"/xfconf/*.xml "$xfconf/"
cp "$src"/applications/*.desktop /usr/share/applications/
for pair in 3:magnus-terminal 4:magnus-files 5:magnus-browser; do
  mkdir -p "/etc/xdg/xfce4/panel/launcher-${pair%%:*}"
  cp "$src/applications/${pair#*:}.desktop" "/etc/xdg/xfce4/panel/launcher-${pair%%:*}/"
done
cp "$src/magnus-applications.menu" /etc/xdg/menus/
mkdir -p /usr/share/desktop-directories
cp "$src/magnus-installed.directory" /usr/share/desktop-directories/
mkdir -p /etc/chromium/policies/managed
cp "$src/chromium-policy.json" /etc/chromium/policies/managed/magnus.json

install -m 755 "$src/magnus-desktop" /usr/local/bin/magnus-desktop
install -m 755 "$src/magnus-browser" /usr/local/bin/magnus-browser
cp "$src/arrange.py" /usr/local/lib/magnus/desktop/arrange.py
mkdir -p /usr/share/doc/magnus-desktop
cp "$src/LICENSES" /usr/share/doc/magnus-desktop/LICENSES
# The recoloring's source, beside the GPL-3.0 icons it changed.
cp "$src/install.sh" /usr/share/doc/magnus-desktop/install.sh
# Nothing starts on its own: no autostarted applets, no notification
# daemon, no screensaver, no session manager.
rm -rf /etc/xdg/autostart/*
