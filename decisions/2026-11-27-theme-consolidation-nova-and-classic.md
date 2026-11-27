# 2026-11-27 — Theme Consolidation (7 Themes → 2 First-Class Themes: Nova + Classic)

## Choice
- Consolidated the `Theme` registry in [`desktop/ui/theme.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/theme.js) from 7 active themes (`nova`, `obsidian`, `obsidian-2`, `obsidian-aurora`, `prism`, `studio`, `aurora`) down to **2 first-class themes**:
  - **`nova`** (default, Dark & Light) — flagship agentic workspace.
  - **`classic`** (Dark & Light) — utilitarian, low-motion, high-contrast fallback.
- Added `LEGACY_STYLE_ALIASES` so any saved preference or programmatic call using a retired style name migrates seamlessly (`obsidian`, `obsidian-2`, `prism`, `studio`, `antigravity`, `companion`, `gemini` → `classic`; `aurora`, `obsidian-aurora` → `nova`).
- Removed the 3 retired theme stylesheets (`theme-prism.css`, `theme-obsidian2.css`, `theme-studio.css`) from `desktop/ui/`, `index.html`, `crates/magnus/src/ui/assets.rs`, `stylesheet-check.mjs`, and `docs/ui-styles.md`.

## Reason
- Maintaining 7 overlapping visual themes multiplied CSS/JS surface area and slowed down every UI iteration.
- Preserving automatic alias resolution guarantees zero regressions for users with saved preferences from earlier builds.
