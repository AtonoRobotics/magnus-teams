# Nova: Greenfield flagship theme, expressive companions, and UX overhaul

**Date:** 2026-11-26

## Decision
Introduce **Nova** (`style: "nova"`) as Magnus's new default theme and overhaul core UI/UX flows across the desktop/web app:

1. **Greenfield `Nova` Theme (`theme.js`, `nova.css`)**
   - Pitch-carbon dark palette (`#0B0C0E` canvas, `#141517` sidebar, `#17181B` matte surface) and clean light palette (`#FFFFFF` canvas, `#F4F6FA` sidebar), verified at >= 4.5:1 WCAG AA contrast across all accents.
   - Included in `STAGE_STYLES` so first-time users get the Autonomous Self-Composing Side+Bottom Bento Stage, icon rail, compact scrollbars, and universally loved subtle floating depth orbs (`particles`) by default.
   - Dissolving midnight cobalt radial halo (`body[data-nova-intro="on"]::before`) on empty new conversations that fades out once work begins, paired with a prismatic-halo editorial showcase (`nova-ux.js`).
   - Whisper-quiet sidebar (hover-only delete actions on pointer devices), airy `15px / 1.64` reading prose, borderless matte Settings & Apps rows, and a Featured Spotlight hero on the Apps catalog.

2. **Selectable Client-Side Companion Avatars (`nova-avatars.js`, `aurora-avatar.js`, `app.js`)**
   - Register three new expressive companions on `AuroraAvatar.define`: **`pip`** (glass-ceramic companion with cursor-tracking eyes, natural blink, scanning working eyes, listening ripples, waiting tilt, and celebratory done bounce), **`mochi`** (pearlescent spirit companion with reactive perk-up ears, soulful catchlight eyes, rosy cheeks, squash-and-stretch bounce, and thinking bubbles), and **`prism`** (luminous faceted star sprite with orbital shard), alongside Magnus's living liquid-glass **`orb`** (`aurora-orb.js`).
   - Persist `avatar` in `Theme.preferences()` and expose a **Companion avatar** selector in Settings > Appearance.

3. **Approvals, Working Moments & Project Hub 2.0 (`asks.js`, `projects.js`, `nova-ux.js`)**
   - Add risk-tier styling (`data-tier`) to approval cards (crimson receipt accent for `financial`/`irreversible`, warm amber for `reversible_write`), move the decline note into a progressive-disclosure `<details class="ask-decline-wrap">`, and add keyboard triage (`A` allow, `D` decline note).
   - Add a calm, dismissible *"While Magnus works"* tip carousel in Nova during active background runs.
   - Upgrade `#/project/:id` into **Project Hub 2.0** with a quiet inline project composer (`data-project-ask`, zero runs until submitted) and a 2-column workspace layout (`.project-hub-grid`).

## Reason
Most autonomous agent products ship stripped-down utilitarian chrome optimized for speed to market. Testers consistently praised Gemini's airy polish and dissolving visual intros, Antigravity's calm structured workspace, Magnus's subtle floating orbs, and expressive animated companions. Making Nova the default unites visual joy with Magnus's verifiable Cedar safety and self-composing stage without adding any build step or breaking existing themes or invariants.
