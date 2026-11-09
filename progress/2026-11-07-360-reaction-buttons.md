# #360 reaction buttons (2026-11-07)

- `desktop/ui/app.js` `msgActions`: `said` is now an explicit kind list mirroring the server's `MAGNUS` (reactions.rs), including `opener` per #359.
- Test: new "react to started tasks" step in `desktop/tests/app-chat-actions.mjs` presses great on the tasks entry, asserts pressed, presses again, asserts taken back. Cards/openers share the same gate line but no harness conversation shows those entries, so the webdriver test covers tasks; a node probe asserts gate parity for all 11 kinds.
- `node --check` clean on both files; the webdriver test itself runs in CI's desktop job.
