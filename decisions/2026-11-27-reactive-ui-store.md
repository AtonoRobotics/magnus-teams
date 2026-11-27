# 2026-11-27 — Reactive UIStore (State-Driven Phase Derivation & Poll Deduplication)

## Choice
- Added [`desktop/ui/ui-store.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/ui-store.js) as a lightweight zero-dependency reactive state store (`UIStore`) loaded before `aurora-core.js`.
- Fed `UIStore` directly from existing data paths: `app.js` (`refresh()`), `bridge.js` (`live()` SSE messages), and `computers.js` (`takeOver()` / `leave()` / `failed()`).
- Updated `aurora-core.js` to derive the canonical 9-state phase via `UIStore.derivePhase()` when hydrated (retaining the DOM fallback for isolated unit tests) and subscribe to `UIStore` updates.
- Updated `aurora-reasoning.js` to subscribe to `UIStore` `tasksById` updates and reuse fresh cached task snapshots (`UIStore.isTaskFresh(r.task)`) instead of issuing redundant `Server.tasks.get()` polls.

## Reason
- Eliminates full-document CSS selector scraping in `AuroraCore.detect()` during live operation.
- Cuts duplicate task polling between `app.js`, `bridge.js` SSE, and `aurora-reasoning.js`.
- Establishes a clean reactive foundation for mobile approval views and subsequent decomposition of `app.js`.
