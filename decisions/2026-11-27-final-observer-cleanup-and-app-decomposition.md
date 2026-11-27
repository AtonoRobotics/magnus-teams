# 2026-11-27 — Final Aurora Observer Cleanup & Extracting app-library.js / app-settings.js (Tier 5 Phase 4)

## Choice
1. **Retire Final Fallback `MutationObserver`s**:
   - Replaced the fallback `MutationObserver`s in [`aurora-reasoning.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/aurora-reasoning.js) and [`aurora.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/aurora.js) with `UIStore` subscriptions and `magnus-render` / `magnus-theme` listeners.
2. **Decompose `app.js` by ~900 lines**:
   - Extracted the Connector Library & Panel Views into [`desktop/ui/app-library.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/app-library.js) (584 lines) and Trash & Settings into [`desktop/ui/app-settings.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/app-settings.js) (314 lines), dropping [`desktop/ui/app.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/app.js) from 3,656 lines down to **2,768 lines** (~1,000 lines smaller since Tier 1!).

## Reason
- Continues steady, zero-regression decomposition of `app.js` into focused `<600L` domain modules while keeping all 91 UI regression checks green.
