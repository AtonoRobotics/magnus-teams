# Obsidian Default UI/UX and Contrast-Guarded Custom Accent Picker

## Context
Testers requested an Obsidian-native UI/UX by default (rather than layering over the legacy Magnus Workspace palette) while preserving the ability to choose any custom accent color.

## Decision
- Replaced `LIGHT` and `DARK` surface tokens in `desktop/ui/theme.js` with Obsidian Alabaster (`#F6F7F9` / `#F1F3F6` / `#FFFFFF`) and Obsidian (`#090A0D` / `#0D0F13` / `#111318`) palettes, and set the default accent (`blue`) to Obsidian Indigo (`#7C8CFF` dark / `#3B5BDB` light).
- Updated `customAccentTokens(hex, mode)` in `desktop/ui/theme.js` to clamp `--accent` to `>= 3.0:1` contrast against `--main` and `--accent-text` to `>= 4.5:1` against both `--main` and `--surface`.
- Updated `desktop/ui/modern-ux.css` and `desktop/ui/computer.css` to use `1px` structural hairlines, unified `44px` top bar, `12px` composer with `--accent-soft` focus halo, `2.5px` left accent bars on active nav and pending decisions, and token-based computer screen framing.
- Upgraded `appearanceSection()` in `desktop/ui/app.js` with a unified swatch row, spectrum custom color picker, `#RRGGBB` hex input, and a "Reset to default" action.

## Reason
Delivers a calm, high-craft Obsidian aesthetic out of the box while giving testers unrestricted custom accent personalization that automatically meets WCAG `4.5:1` contrast.
