# 2026-11-27 — Retire Remaining Aurora MutationObservers & Extract app-polling.js (Tier 4 Phase 4)

## Choice
1. **Retire Standalone `MutationObserver` Fallbacks**:
   - Removed the remaining `new MutationObserver(...)` fallbacks in `aurora-alerts.js`, `aurora-compact.js`, `aurora-pages.js`, and `aurora-explore.js`, replacing them with `UIStore.subscribe("*", ...)` and `window.addEventListener("magnus-render", ...)` alongside `AuroraCore` hooks.
2. **Extract `desktop/ui/app-polling.js`**:
   - Extracted `seenKey`, `load`, `save`, `pollInbox`, and `pollVersions` from `app.js` into [`desktop/ui/app-polling.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/app-polling.js) (registered in `index.html` and `crates/magnus/src/ui/assets.rs`), continuing the incremental, zero-regression decomposition of `app.js`.

## Reason
- Consolidates DOM observation into `AuroraCore` + reactive `UIStore`/`magnus-render` events instead of scattering redundant subtree observers across individual modules.
- Keeps new modules well under 800 lines while steadily shrinking `app.js`.
