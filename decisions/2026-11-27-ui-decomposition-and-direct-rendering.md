# 2026-11-27 — UI Decomposition & First-Class Render Hooks (Tier 3 Phase 4)

## Choice
1. **First-Class Render Hooks (`app.js`, `nova-ux.js`, `nova-frame.js`)**:
   - Added a microtask-coalesced `magnus-render` event emitted by `paint()` in [`desktop/ui/app.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/app.js).
   - Retired the global `MutationObserver` fallbacks in [`desktop/ui/nova-ux.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/nova-ux.js) and [`desktop/ui/nova-frame.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/nova-frame.js), subscribing them directly to `UIStore.subscribe("*", ...)` and `magnus-render`.
2. **Decomposing `aurora-stage.js` below 800 lines & shrinking `app.js`**:
   - Extracted canvas shell, decision preview, thread history compaction, completed-answer annotations, and word-by-word answer streaming into [`desktop/ui/aurora-stage-view.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/aurora-stage-view.js), bringing [`desktop/ui/aurora-stage.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/aurora-stage.js) from 1,011 lines down to **773 lines** (strictly under the 800-line ceiling).
   - Extracted file chips, verdict bars, uploads, project move controls, previews, and glimpses from `app.js` into [`desktop/ui/app-files.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/app-files.js) while preserving all existing globals and test contracts.

## Reason
- Eliminates redundant subtree `MutationObserver` churn in Nova chrome.
- Brings `aurora-stage.js` into full compliance with the 800-line Code Standard ceiling and begins incremental, zero-regression decomposition of `app.js`.
