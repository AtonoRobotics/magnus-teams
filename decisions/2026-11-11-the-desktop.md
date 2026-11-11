# The computer's desktop: XFCE themed as Magnus

Spec rev 145, "The desktop". Built in `sandbox/desktop/` (installed into the base image by `sandbox/Dockerfile`), the guest agent (`start_display`), the kernel (`crates/magnus/src/computer/desktop.rs`) and the app (`followDesktopLook` in `desktop/ui/app.js`). It replaces fluxbox and xterm.

## What it is

XFCE 4.18 on X11 (Xvfb) at 1280×800: xfwm4, xfce4-panel, xfdesktop, xfsettingsd, Thunar and xfce4-terminal, from Debian bookworm. `magnus-desktop start` starts it once, from both the screenshot path (`boundary::computer_command`) and the takeover's VNC (`start_display`), and later calls wait on a lock for the first. The children close the lock's file descriptor, or a second start would wait forever. The session's settings live in `/tmp/magnus-desktop`, so every boot starts from the base image's look in `/etc/xdg`, and nothing of the desktop's own state lands on the user volume. Commands the agent runs get the session's bus address, so an app it starts takes the session's settings.

An older image without `magnus-desktop` still gets its bare display and window manager from the screenshot command, so a kernel that updates before its image does not lose screenshots.

## Look

- Colors are the app's own tokens (`desktop/ui/theme.js`, light and dark, blue accent), as the spec says ("the app's colors"). The Desktop artboard draws the chrome in a navy palette (`#0E1626` bar in dark); where the artboard and the spec differ the spec wins (the design README says so), so the bar is the app's `#19191C`/`#FAFAF8`. The difference is within the acceptance check's tolerance against the artboard. `sandbox/desktop/test_desktop.py` fails if a desktop color is not one of the app's.
- The wallpaper is the Wallpaper artboard exactly, drawn as SVG by `make_theme.py` and rendered at 2× by librsvg at build time, as the design README asks.
- The GTK theme is GTK's own Adwaita (imported from GTK's resources) with the app's colors over it, so no other theme's files are copied. The xfwm4 theme's pictures are drawn by `make_theme.py`: a 38-pixel title bar and 12-pixel rounded top corners as the artboard has, rounded bottom corners, the artboard's minimize, maximize and close.
- One top bar, 34 pixels: the Magnus menu (the star, "Magnus"), the three pinned apps as icons, the open windows with their names, and the clock (`Wed 11 Nov 6:26`, the artboard's format). XFCE has no dock that is both pinned and a window list, so the pinned apps are launchers and the open windows follow them; the artboard's labels on the pinned buttons belong to the window list.
- The menu is `magnus-applications.menu`: Terminal, Files, Browser, then "Installed by Magnus", Office, Graphics, Development and Other, with no settings dialogs.
- Inter for the interface and window titles, JetBrains Mono in the terminal. Icons are Papirus (Papirus-Light and Papirus-Dark by theme) with the folder blue and the symbolic icons' gray substituted for the app's colors.
- Thunar shows a warning bar to root on every window. The computer runs as root by design (it is the sandbox), so the theme collapses that bar in Thunar only.

## Light and dark follow the person

The app's theme lives in the browser, so the app tells the kernel: when the person's effective look (light or dark, after "Match computer") differs from their computer's, it sends `PUT /v1/computers/{id}/theme`. The kernel records a `desktop_theme` computer change (a new event kind) and, if the computer is running, runs `magnus-desktop theme light|dark` in it, which switches the GTK theme, window theme, icons, terminal colors and wallpaper live. Every wake runs it too, so a change made while the computer slept is applied. A new computer is dark.

Only a person's own computer follows their theme. A team's computer has many people and no one theme, so it keeps the default (dark): switching it to whoever looked last would change the screen under the agent, which the spec's "Calm for the agent too" rules out.

## Calm

No compositor, shadows, zoom, moving outlines, GTK animations, blinking cursors, bells or event sounds; no notification daemon, screensaver, lock screen or session manager; Xvfb without screen blanking or power saving; Thunar makes no thumbnails (they arrive late and change the screen). `arrange.py` places windows: one window maximized; two snapped to halves, the older on the left; a third or later opens maximized and leaves the others. Dialogs are left to xfwm4, which centres them.

## Browser

`magnus-browser` runs Chromium with a profile in `/tmp`, and a managed policy turns off saved passwords, sign-in, sync and autofill: it holds no logins, as the spec says. It runs with `--no-sandbox` because the guest is root; the computer is the sandbox. It trusts the computer's proxy certificate the kernel installs.

## Apps the agent installs

After an install is recorded, and after each app the manifest replays onto a new overlay, the kernel runs `magnus-desktop app-installed NAME`. A package's own menu entry is already in the menu; a program without one, found by the app's name, gets an entry in "Installed by Magnus". An app with neither is left out rather than guessed.

## Licences

Every theme, font and icon set is licensed for redistribution. The list, with each source and licence, ships in the image at `/usr/share/doc/magnus-desktop/LICENSES` (from `sandbox/desktop/LICENSES`):

- Magnus-Light and Magnus-Dark (GTK and xfwm4), the wallpaper and the star: Magnus's own, made by `make_theme.py`.
- Adwaita GTK base theme: in GTK 3 (Debian libgtk-3-common 3.24.38), LGPL-2.0-or-later.
- Inter: Debian fonts-inter 4.0~beta7, github.com/rsms/inter, SIL OFL 1.1.
- JetBrains Mono: Debian fonts-jetbrains-mono 2.242, github.com/JetBrains/JetBrainsMono, Apache-2.0 as Debian ships this version (later versions are OFL 1.1).
- Papirus icons: Debian papirus-icon-theme 20230104, github.com/PapirusDevelopmentTeam/papirus-icon-theme, GPL-3.0. The recolored icons are a modification under the same licence; the script that made it ships beside the list.
- Adwaita cursors and fallback icons: Debian adwaita-icon-theme 43, LGPL-3.0 or CC-BY-SA-3.0 (some CC-BY-SA-4.0).
- hicolor: Debian hicolor-icon-theme 0.17, GPL-2.0-or-later.

## Acceptance

`ci/acceptance.sh desktop_look` (`tests/scratch/tests/desktop_look.rs`, KVM, Strix Beta) boots a computer from the image, screenshots it through `/v1/computers/{id}/screen` in dark and then light, and compares with the artboards rendered by `tests/scratch/reference/render-desktop.mjs` (committed as `desktop-dark.png` and `desktop-light.png`): the bar's height (34 ± 2), its color against the app's token and the artboard's, the wallpaper's mean difference from the artboard and its share of far pixels, the mark's looser difference (fonts render differently), and the menu, three pinned apps and clock drawn on the bar. Then one Terminal fills the screen, and Terminal and Files share it half and half.

A new base image invalidates computers' snapshots and overlays; the rebuild path replays the manifest, as the spec has it.
