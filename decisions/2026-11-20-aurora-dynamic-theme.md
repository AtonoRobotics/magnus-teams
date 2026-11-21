# 2026-11-20 — Aurora dynamic theme across desktop and web UI

## Choice
Add **Aurora** (`style: "aurora"`) as a fifth selectable theme in `desktop/ui/theme.js`, implemented via an isolated stylesheet (`desktop/ui/aurora.css`) and self-gating presence/lighting script (`desktop/ui/aurora.js`), while leaving `obsidian`, `obsidian-2`, `prism`, and `studio` completely untouched.

## Reason
- Scoping all frosted glass, living ambient light fields, rotating conic-gradient execution beams, masked entry animations, and pointer light-catch rules under `html[data-style="aurora"]` lets the entire app (navigation, right sidebar, computer panel, composer, hero, team workspaces, settings, and dialogs) feel dynamic without regressing any existing theme.
- Gating the 60fps `<canvas>` presence orb in `aurora.js` on `document.documentElement.dataset.style === "aurora"` and `data-motion` ensures zero runtime or GPU cost when another theme or reduced/off motion is selected.
- Verified with `theme-check.mjs` (≥ 4.5:1 contrast across all Aurora light/dark accents), `preferences-check.mjs`, `stylesheet-check.mjs`, `globals-check.mjs`, and `ci/ratchet.sh size`.
