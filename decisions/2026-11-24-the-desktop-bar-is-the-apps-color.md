# The computer's desktop follows the app's theme and its own wallpaper, not the 11-11 artboards

**Choice.** The desktop acceptance check (tests/scratch/tests/desktop_look.rs)
checks the top bar's color against the app's theme token only, and its
reference images (tests/scratch/reference/render-desktop.mjs) draw the
Desktop artboard over the wallpaper sandbox/desktop/make_theme.py draws, not
over the Wallpaper artboard. It still compares the bar's height, the
wallpaper, the mark and the menu with that reference.

**Reason.** The artboards (docs/design/team-and-computer, 2026-11-11) color
the bar #0E1626 and draw a star wallpaper. The owner replaced the wallpaper
with Magnus Obsidian on 2026-11-16 (f5e9bce), and the app's themes were redone
after the artboards (Obsidian 2 and Aurora, 2026-11-17 and 2026-11-20); the
computer's desktop follows them (#121215 bar in dark). A desktop that matches
those cannot also match the older artboards, so the check failed on every run.
The later choices are the ones the owner made; the mockups are left as they
are. The check keeps its point: the built image must show the wallpaper it
means to, at its size, with the bar, menu, pinned apps and clock over it.
