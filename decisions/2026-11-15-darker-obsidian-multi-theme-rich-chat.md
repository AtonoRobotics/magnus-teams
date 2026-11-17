# Darker Obsidian Background, Multi-Theme Accents, and Unboxed Rich Chat

## Context
After reviewing interactive UI mockups (`magnus_serious_mockup.html` and `magnus_rich_chat_mockup.html`), the owner approved transitioning Magnus to an Obsidian-inspired serious minimalist aesthetic with:
1. A darker obsidian dark mode (`#07080B` / `#0B0C0F` / `#101216`).
2. Additional curated color themes (`Indigo`, `Emerald`, `Amber`, `Rose`) alongside `Blue`, `Teal`, `Clay`, and `Graphite`.
3. Unboxed editorial prose, `tabular-nums` hairline tables, proportional bar primitives, and a single focal surface for pending decisions.

## Decision
- Updated `DARK` in `desktop/ui/theme.js` to deeper obsidian surfaces (`bg: #07080B`, `side: #0B0C0F`, `main: #101216`, `surface: #16181F`) and synced `PALETTES["dark"]` in `sandbox/desktop/make_theme.py`.
- Added `indigo`, `emerald`, `amber`, and `rose` to `ACCENTS` in `desktop/ui/theme.js`, each verified to meet `>= 4.5:1` WCAG contrast across solid and glass looks via `desktop/tests/theme-check.mjs`.
- Updated `desktop/ui/modern-ux.css` to flatten nested box shadows, render `.hello-hero` starter prompts as clean hairline rows, style `.answer-body table` and `blockquote` as unboxed editorial elements, and tighten `.say` and `.msg.me` radii to `16px`.

## Reason
Reduces cognitive load by eliminating nested card chrome, keeps pending approvals as the clear visual focal point, and gives users 8 high-contrast color themes on a true dark obsidian canvas.
