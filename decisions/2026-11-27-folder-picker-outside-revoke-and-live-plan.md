# 2026-11-27 — Folder Picker, Outside-Agent Admission Revoke & Live Plan Card (Tier 5 Phases 2–3)

## Choice
1. **Folder Picker (`bridge.js`, `computers.js`)**:
   - Added `Magnus.pickFolder()` (with `window.showDirectoryPicker` support on Chromium browsers and graceful fallback messaging) and a `"Browse folder…"` button (`data-act="local-browse-folder"`) beside local computer quick-add chips.
2. **Outside-Agent Admission Revoke (`server.js`, `outside-agents.js`)**:
   - Added `Server.outsideAgents.revoke(id, to)` (`DELETE /v1/outside-agents/{id}/admit`) and a `"Stop letting it ask me"` button (`data-act="outside-revoke"`) when an outside agent is admitted, allowing users to halt inbound requests without disconnecting outbound tasks.
3. **Inline Live Plan Card (`plan-surface.js`, `personal-chat.js`)**:
   - Added `PlanSurface.trackTask(taskId, steps)` and `PlanSurface.renderLiveCard(taskId)` subscribed to `UIStore` `tasksById` so approved plans tick off steps live (`todo` → `active` → `done`) as Magnus executes them.

## Reason
- Closes the remaining UX gaps in local computer setup, A2A admission control, and live plan progress visibility.
