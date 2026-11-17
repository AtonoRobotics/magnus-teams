# Separate Obsidian & Companion Themes and Companion Right Sidebar

## Context
Testers asked for:
1. A default theme that avoids pill-heavy chrome and saturated default accents, using a sleek monochrome Obsidian baseline with tight architectural radii while retaining the custom accent picker.
2. A completely redesigned right inspector (`#detail`) modeled on the Grobot + Muse companion patterns (worker identity header, live computer preview, collapsible Heartbeat/schedules bar, chronological timeline, expandable approval history & guardrails, typed file filtering, and an in-panel Apps tab).
3. Both visual aesthetics preserved as separate selectable themes so users can switch between tight Obsidian and soft rounded Companion at any time.

## Decision
- Added `Theme.STYLES` (`"obsidian"` default, `"companion"` selectable) and `Theme.STYLE_TOKENS` in `desktop/ui/theme.js` driving shared CSS tokens (`--r-xs`, `--r-sm`, `--r-md`, `--r-lg`, `--r-xl`, `--r-pill`, `--r-send`, `--r-avatar`, `--r-composer`, `--focus-ring`) and `html[data-style]`.
- Added monochrome preset `graphite` (`Monochrome (Default)`, `#ECEFF4` dark / `#111318` light) as the default accent for `obsidian`, while switching to `companion` auto-shifts `graphite -> blue` (`Indigo`, `#7C8CFF` / `#3B5BDB`) and switching back shifts `blue -> graphite` without overriding explicit presets or custom hex colors.
- Hardened `customAccentTokens()` in `desktop/ui/theme.js` to ensure `accent-fg` on `accent` reaches `>= 4.5:1` and `accent-text` reaches `>= 4.5:1` across `main`, `surface`, `raised`, and `accent-soft`.
- Replaced hardcoded border radii and focus rings across all eight `desktop/ui/*.css` sheets and `desktop/ui/compose.js` with `--r-*` and `--focus-ring`, gave `.panel-tabs` flush underline tabs by default (`obsidian`) and pill-track tabs under `html[data-style="companion"]`, and removed `backdrop-filter: blur(8px)` from `.settings-modal::backdrop` and `.wizard-modal::backdrop`.
- Redesigned `#detail` in `desktop/ui/app.js` with `companionHeader()`, 4 companion tabs (`Activity`, `Approvals`, `Files`, `Apps`), dismissible suggested routines card, live Cloud Computer preview card, collapsible Heartbeat & schedule summary, day-grouped `Recent work`, expandable recent decisions & `Safety Guardrails`, `PANELS.rules`, and `All | Pages | Media | Files` filtering with `This week` / `Older` grouping in `filesView()`.
- Added a quick `Theme` link in the companion header (`#/settings/appearance`) and a `Theme` radio group to `appearanceSection()`.
- Replaced remaining browser `confirm()` and `prompt()` calls in `desktop/ui/computers.js` and `desktop/ui/team-panel.js` with styled `<dialog class="ask">` helpers (`ask()` and `askText()`).

## Reason
Delivering two clean, token-driven themes keeps the default UI sharp, monochrome, and distraction-free like Obsidian while still letting people choose the softer Grobot + Muse Companion silhouette, and uniting both styles with the reorganized companion right sidebar makes Magnus's live status, computer screen, schedules, approvals, files, and apps immediately scannable without leaving the conversation.
