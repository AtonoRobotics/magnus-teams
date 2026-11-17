# Bundled SVG App Logos & Visual Polish for Apps Cards

## Decision
Replaced the flat 2-letter monogram tiles in Apps with bundled inline SVG brand marks (`desktop/ui/app-logos.js`) and restructured connector and local program cards (`desktop/ui/app.js`, `desktop/ui/programs.js`, `desktop/ui/pages.css`).

## Details
- Added `AppLogos.svg(name)` in `desktop/ui/app-logos.js` (registered in `desktop/ui/index.html` and `crates/magnus/src/ui/assets.rs`) covering all 17 built-in catalog connectors (`asana`, `cloudflare`, `github`, `gitlab`, `google-calendar`, `google-chat`, `google-drive`, `google-gmail`, `huggingface`, `microsoft-calendar`, `microsoft-onedrive`, `microsoft-outlook`, `sentry`, `shopify`, `slack`, `supabase`, `vercel`) and 4 local desktop programs (`blender`, `freecad`, `fusion`, `solidworks`), keeping the 2-letter fallback for custom/org connectors.
- Updated `connectorCard(c)` and `programCard(p, later)` to use a Header (emblem + title/byline + action) / Body (2-line clamped description + up to 2 ability tags) / Footer (connection status dot + `Details →`) structure.
- Added Obsidian hover borders, Prism borderless tonal elevation, `.ctile` inner sheen, and compact `[data-panel="apps"]` styling for the right sidebar.
