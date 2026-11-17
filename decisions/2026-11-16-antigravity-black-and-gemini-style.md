# Pure Black Antigravity Palette, Gemini Style, and Colored Tool/File Details

## Context
The dark theme (`DARK` in `desktop/ui/theme.js`) still carried a blue/slate tint (`#090A0D`–`#242834`, `#A6B0C3`, `#8892A6`), and the secondary style was named `companion`. Testers also benefit from colored file-type badges and colored workflow/tool icons rather than monochrome text tags.

## Decision
1. Replaced `DARK` in `desktop/ui/theme.js` with pure neutral black/charcoal (`#000000`, `#0A0A0A`, `#0E0E0E`, `#151515`, `#1C1C1C`) and neutral grey text/borders (`#F5F5F5`, `#B3B3B3`, `#8E8E8E`, `#242424`, `#363636`), and updated `graphite` to pure neutral `#F5F5F5`/`#111111`.
2. Replaced the `companion` style with `gemini` (`STYLES`, `STYLE_TOKENS.gemini`, `data-style="gemini"` across `base.css`, `chat.css`, `layout.css`, `pages.css`, `app.js`, and tests), migrating any stored `style: "companion"` preference to `"gemini"` in `Theme.preferences()`.
3. Added colored workflow icon chains (`.card-flow`, `.flow-node`, `.tone-*`) and colored tool badges to `desktop/ui/discover.js`, plus colored `CODE` and `JSON` file-type badges alongside `PDF`, `CSV`, `MD`, and `IMG` in `filesView()` (`desktop/ui/app.js`, `desktop/ui/pages.css`).

## Reason
Removing the blue tint achieves true Antigravity black-and-grey contrast (5.18:1+ across all looks), offering `Gemini` as the soft rounded alternative matches the user's mental model, and colored tool/file-type badges make workflows and files immediately scannable.
