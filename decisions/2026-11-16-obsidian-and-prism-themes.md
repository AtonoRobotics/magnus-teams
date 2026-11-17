# Rename UI Themes to Obsidian and Prism

## Decision
Renamed Magnus's two UI themes across the desktop UI, tests, and documentation:
- `antigravity` / `Antigravity` → `obsidian` / `Obsidian` (the default pure-black architectural theme with tight radii and monochrome `graphite` accent)
- `gemini` / `Gemini` → `prism` / `Prism` (the soft rounded theme with `999px` pill controls, floating user bubbles, and `blue` Indigo default accent)

## Reason
The owner asked to replace the internal/external `Antigravity` and `Gemini` theme names with distinct product theme names across the UI and documents, choosing **Obsidian** and **Prism**.

## Implementation
- Updated `Theme.STYLES`, `Theme.STYLE_TOKENS`, `Theme.DEFAULTS`, and `Theme.preferences()` in `desktop/ui/theme.js`, keeping automatic migration from legacy `"antigravity"`, `"gemini"`, and `"companion"` values.
- Updated `companionHeader()` and `appearanceSection()` in `desktop/ui/app.js`, and all `html[data-style="prism"]` selectors in `desktop/ui/base.css`, `desktop/ui/chat.css`, `desktop/ui/layout.css`, and `desktop/ui/pages.css`.
- Updated `desktop/tests/theme-check.mjs`, `desktop/tests/preferences-check.mjs`, `desktop/tests/companion-panel-check.mjs`, and `desktop/tests/app-looks.mjs`, and replaced theme references in `decisions/`. (Model provider references to `adapter-gemini` and Gemini LLM benchmarks were left intact.)
