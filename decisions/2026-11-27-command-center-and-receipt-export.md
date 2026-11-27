# 2026-11-27 — Live Command Center & Verifiable Receipt Export (Tier 4 Phase 3)

## Choice
1. **Live `⌘K` Command Center (`app.js`)**:
   - Extended the Command Palette item list with live pending approvals (`Approval: ...` jumping straight to the approval card) and active/recent tasks (`Live Task` / `Task` jumping straight to task detail/replay), backed directly by `App.tasks` and `needsMe()` with zero extra network calls.
2. **Verifiable Receipt Export (`evidence.js`)**:
   - Added `receiptsToMarkdown()` and `receiptsToJson()` plus 1-click **Copy proof** and **Export JSON** buttons beside `"What was done"` in `evidenceSection()`.

## Reason
- Lets keyboard-first users jump directly to any waiting approval or running task in two keystrokes.
- Turns Magnus's server-verified post-action read-backs into portable compliance/audit proof that users can paste into tickets, Slack, or emails.
