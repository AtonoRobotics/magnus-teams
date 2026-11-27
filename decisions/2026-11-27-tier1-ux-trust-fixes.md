# Tier 1 UX & trust fixes

**Date:** 2026-11-27

## Decision
Land eight targeted UX and operational trust fixes across the desktop/web UI:

1. **Blocking approvals bypass Do Not Disturb (`notices.js`, `app.js`):** Forward `blocking: !!i.blocking` into `Notices.show` and skip DND suppression when `s.blocking` is true, matching the Notification Center promise that only non-blocking pop-ups are silenced.
2. **Persistent Admin Kill Switch (`admin.js`, `nova-frame.js`, `app.js`):** Keep `[data-admin-kill]` in `<aside id="detail">` for compatibility and sync a second persistent `[data-admin-kill-top]` mount inside `.nova-frame-bar` and `#workspace-bar`, so admins have a 1-click Emergency Stop even when the right panel is closed.
3. **Spend requests & Admin accuracy (`settings-sections.js`, `admin-spend.js`, `admin-agents.js`):** Pass a valid `{ error, spend_limit }` payload from Settings' `"Request more"` button to `offerMoreSpend`; remove retired `"Team Admin"` channel-limit copy from Admin Spend; label agents with no running tasks `"Idle"` instead of `"Working"`.
4. **VNC takeover listener cleanup & step-recording handback (`computers.js`):** Bind all `#display` canvas pointer/keyboard listeners with a per-takeover `AbortController` aborted on `leave()` and `failed()`, preventing N× duplicate events on repeat takeovers. Finish active step recording before handing back when `"I'm done"` is clicked.
5. **Composer ergonomics & Edit-cancel preservation (`chat-composer.js`, `app.js`, `aurora-rail.js`, `personal-chat.js`, `message-controls.js`):** Stop propagation on composer `⌘/Ctrl+B/I/E/K` formatting shortcuts and respect `e.defaultPrevented` in global shortcut listeners; allow submitting file-only messages; return `false` when `"Edit & resend"` is cancelled so the inline edit box and draft are preserved.
6. **Guarded approval triage & complete Shortcuts modal (`nova-ux.js`, `app.js`):** Gate bare `A`/`E`/`D` approval triage on `isNova()`, `!e.defaultPrevented`, no open `<dialog[open]>`, and a visible ask card; document composer formatting and approval triage in `#shortcuts-modal`.
7. **Cleaner routines & task filtering (`tasks-schedules.js`, `onboarding.js`):** Toggle cadence inputs dynamically by `select[name="kind"]` instead of showing all fields at once; add the `"Waiting"` filter chip in Tasks; fix Discovery Wizard weekday proposals to use `weekdays_at` instead of `daily_at`.
8. **Plain-language copy & Trash access (`nova-pages.js`, `nova-ux.js`, `settings-sections.js`):** Remove internal `"Cedar"` engine jargon from user-facing Nova copy and add a discoverable `"Open Trash"` link in `Settings → Account & Privacy` without re-adding a Nova sidebar row.

## Reason
An in-depth UI/UX audit identified these as concrete, high-impact friction and trust bugs where controls either silently failed (spend requests, file-only sends, DND suppressing blocking approvals, hidden Kill Switch) or behaved unexpectedly (duplicate VNC events, shortcut collisions, discarded edits). Verified by `desktop/tests/tier1-ux-fixes-check.mjs`.
