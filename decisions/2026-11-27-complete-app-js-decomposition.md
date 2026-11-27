# 2026-11-27 — Complete app.js Decomposition Below 800 Lines

## Choice
- Completed decomposing [`desktop/ui/app.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/app.js) from 3,757 lines down to **598 lines** — bringing **every single `.js` and `.css` file in `desktop/ui/` strictly below the 800-line ceiling**:
  - `app.js` (598L): core state (`App`), `ReadingAnchors`, `Resources`, `paint()`, `go()`, `refresh()`, `modelPicker()`.
  - `app-shell.js` (681L): workspace shell, sidebar navigation, Command Palette (`⌘K`), Shortcuts modal, document wiring.
  - `app-files.js` (92L): file chips, verdict bars, uploads, project move controls, previews, glimpses.
  - `app-polling.js` (63L): `seenKey`/`load`/`save`, `pollInbox`, `pollVersions`.
  - `app-chat.js` (399L): personal chat turns, tasks, and home view.
  - `app-inbox.js` (497L): inbox cards, `respond()`, approvals, and activity views.
  - `app-panel.js` (298L): right side panel beside the conversation.
  - `app-library.js` (584L): connector catalog, Apps Library, and panel view wiring.
  - `app-settings.js` (314L): Trash and Settings modal views.
  - `app-boot.js` (317L): app boot, lifecycle, polling schedule, and updates.
- Added [`desktop/tests/app-bundle.mjs`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/tests/app-bundle.mjs) so frontend tests read the split `app*.js` modules in browser load order cleanly.

## Reason
- Satisfies the 800-line Code Standard across 100% of `desktop/ui/` while preserving every global symbol and keeping all 92 UI regression checks passing.
