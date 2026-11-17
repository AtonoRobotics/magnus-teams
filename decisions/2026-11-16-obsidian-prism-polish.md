# Polish Obsidian & Prism Themes and Fix Hero Clipping

## Problem
Screenshot review (`image.png`) showed three visual/layout issues:
1. `.thread:has(.hello-hero) { justify-content: center; }` clipped the top of the hero banner and greeting above `scrollTop = 0` on shorter viewports.
2. `.hello-banner` and `.card-media` had hard borders/seams that made artwork look pasted in, and each hero card carried 6–7 competing colored pills.
3. **Prism** shared the exact same `#000000`/`#0E0E0E` surfaces and left-aligned hero as **Obsidian**, rather than having Gemini-style soft tonal slate surfaces, centered hero, borderless cards, pill navigation, and elevated 24px composer.

## Changes
- **`desktop/ui/theme.js`**: Added dedicated `PRISM_DARK` (`#131314` / `#1E1F20` / `#282A2C`) and `PRISM_LIGHT` (`#FFFFFF` / `#F0F4F9` / `#E9EEF6`) palettes, and refined `DARK` (`#000000` / `#09090B` / `#121215`) for crisp Obsidian contrast.
- **`desktop/ui/discover.js`**: Simplified compact hero card footers to a quiet muted tool summary (`hello-card-meta`) + hover `Try →` cue instead of duplicate colored pills.
- **`desktop/ui/chat.css`**: Fixed hero clipping (`justify-content: flex-start` on `.thread:has(.hello-hero)` + `margin: auto` on `.hello-hero`), added edge/bottom gradient fades to `.hello-banner` and `.card-media`, tightened hero spacing, refined Obsidian `.say` surface, and added Gemini-style Prism rules (centered hero, borderless tonal cards, elevated 24px composer, soft user bubbles).
- **`desktop/ui/layout.css`**: Added Prism borderless sidebar and pill-shaped active nav styling.

## Verification
All desktop UI checks (`theme-check.mjs`, `preferences-check.mjs`, `companion-panel-check.mjs`, `discover-check.mjs`, `stylesheet-check.mjs`, `globals-check.mjs`, `wording-check.mjs`, `chat-view-check.mjs`, `onboarding-check.mjs`) and `ci/ratchet.sh size` pass.
