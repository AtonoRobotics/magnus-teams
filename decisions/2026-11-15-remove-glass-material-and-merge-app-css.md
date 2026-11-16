# Remove Legacy Glass/Material Theme System and Merge `modern-ux.css` into `app.css`

## Context
Previously, Antigravity UI styling was layered on top of the legacy Magnus Workspace theme system via `desktop/ui/modern-ux.css`, while `desktop/ui/theme.js`, `desktop/src-tauri/src/look.rs`, and `desktop/tests/` still carried the `GLASS` palette, `MATERIALS`, `window_vibrancy` (Mica/Acrylic), and `material: "glass"` preference logic.

## Decision
- Merged `desktop/ui/modern-ux.css` into `desktop/ui/app.css` (removing superseded duplicate rules) and removed `modern-ux.css` from `desktop/ui/index.html` and `crates/magnus/src/ui/assets.rs`.
- Removed `GLASS`, `MATERIALS`, `GLASS_SURFACES`, `reduceTransparency`, and `material` from `desktop/ui/theme.js`; `Theme.scope()` migrates any saved `localStorage` record containing `material` so legacy `material: "glass"` preferences are cleanly stripped.
- Removed `window_vibrancy`, `glass`, `reduce_transparency`, and `set_material` from `desktop/src-tauri/src/look.rs`, `desktop/src-tauri/src/lib.rs`, and `desktop/ui/bridge.js`, replacing `set_material` with `set_background(dark)` using Antigravity `SOLID_LIGHT` (`#F6F7F9`) and `SOLID_DARK` (`#090A0D`).
- Updated `desktop/tests/theme-check.mjs`, `desktop/tests/preferences-check.mjs`, and `desktop/tests/native-material-check.mjs` to verify the Antigravity theme and migration directly without glass/material fixtures.

## Reason
Eliminates CSS specificity conflicts and removes the retired Glass/Material theme system at the root rather than recoloring or overriding it.
