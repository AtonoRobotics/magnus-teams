# Strict theme isolation and declarative theme registry (Nova default)

**Date:** 2026-11-27

## Decision
1. **Declarative Theme Registry (`desktop/ui/theme.js`):** Consolidate all theme palettes, radii, default accents, panel surface modes, and UI capability flags into a single `THEMES` registry with **Nova** registered first and set as the default (`DEFAULTS.style = "nova"`). Export `Theme.get(style)`, `Theme.has(feature, style)`, `Theme.active()`, and `Theme.register(def)` while preserving full backward compatibility with existing `Theme.*` exports.
2. **Capability-driven shared JS (`app.js`, `aurora-rail.js`, `aurora-pages.js`, `aurora-avatar.js`, `discover.js`):** Replace hardcoded `dataset.style === "nova"` checks in shared modules with declarative `Theme.has(...)` / `Theme.get(...)` capability queries.
3. **Zero theme bleed in base CSS:** Extract all theme-specific `html[data-style="prism"|"obsidian-2"|"studio"]` rules out of `base.css`, `layout.css`, `chat.css`, and `pages.css` into isolated per-theme stylesheets (`theme-prism.css`, `theme-obsidian2.css`, `theme-studio.css`), registered in `index.html`, `crates/magnus/src/ui/assets.rs`, `desktop/tests/stylesheet-check.mjs`, and `docs/ui-styles.md`.
4. **Automated guardrail (`desktop/tests/theme-isolation-check.mjs`):** Enforce zero `[data-style=...]` selectors in base CSS, strict single-scope selectors in every theme stylesheet, `Theme.DEFAULTS.style === "nova"`, and zero hardcoded `dataset.style === "nova"` checks in shared JS modules.

## Reason
Base stylesheets previously mixed overrides for Prism, Obsidian 2, and Studio into shared layout and component files, while shared JS modules branched on hardcoded theme names. Isolating every theme's CSS and centralizing theme capabilities in `theme.js` prevents cross-theme regressions, keeps Nova cleanly isolated as the flagship default, and makes adding or retiring themes safe and mechanical.
