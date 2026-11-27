# 2026-11-27 — P0 Runtime Wiring Fixes (UIStore task_id/needsMe, MobileApprove Server.inbox, Live Plan Card Mount, Desktop pickFolder)

## Choice
1. **`UIStore` real contract alignment (`ui-store.js`, `app.js`)**:
   - Updated `UIStore.setTask()` and `UIStore.setTasks()` to key on `task.task_id || task.id` so live `/v1/tasks` snapshots are indexed properly.
   - Updated `app.js` `refresh()` to publish `needsMe()` into `UIStore.asks` instead of nonexistent `App.asks`.
2. **`#/approve` real API & respond() wiring (`mobile-approve.js`, `app-polling.js`)**:
   - Rewired `MobileApprove` to call `Server.inbox.list()`, `Server.inbox.batch(ids, { decision: "approve", scope: "once" })`, filter `status === "open"` via `needsDecision()`, and delegate individual `button[data-decision]` clicks to `respond(btn, main)`.
   - Added the compact Admin **Emergency Stop** button (`[data-mobile-admin-kill]`) on `#/approve` and mobile deep-links (`#/approve?ask=<id>`) on blocking notifications in `app-polling.js`.
3. **Inline Live Plan Card mount (`app.js`)**:
   - Mounted `PlanSurface.renderLiveCard(e.task_id)` directly inside `case "turn":` in `entry()` so approved plans render inline and tick off live from `UIStore`.
4. **Desktop bridge `pickFolder()` (`bridge.js`)**:
   - Added `pickFolder()` to the Tauri desktop bridge alongside the web bridge.

## Reason
- Post-implementation audit caught four silent wiring mismatches where unit tests only checked regexes or synthetic payloads instead of live Magnus contracts (`task_id`, `Server.inbox`, `respond(btn, root)`). Added end-to-end behavioral tests in `desktop/tests/p0-wiring-fixes-check.mjs` to lock in the real contracts.
