# Takeover ergonomics, per-sub-agent stop, and adaptive launchpads

**Date:** 2026-11-27

## Decision
1. **Bidirectional VNC clipboard sync (`rfb.js`, `screen.js`, `computers.js`):** Decode RFB `ServerCutText` (type `3`) into `onCut` (writing to the local clipboard), implement `Rfb.prototype.cut()` (`ClientCutText`, type `6`), bind `paste` on the takeover canvas, and expose a **Paste** button in the takeover control strip.
2. **Per-sub-agent inline Stop (`subagents.js`, `app.js`):** Add an inline `Stop` button (`data-stop-subagent`) to running/suspended sub-agent rows wired directly to `Server.tasks.cancel(id)`.
3. **Local computer folder quick-add & offline guards (`computers.js`):** Add quick-add folder chips (`~/Projects`, `~/Documents`, `~/Desktop`, `~/Downloads`) under `#lc-folders` and disable local file Download/Delete controls when the computer is offline.
4. **App-aware Nova launchpads (`nova-ux.js`):** Select the 3 Home launchpads dynamically from `App.connectors` (surfacing Mail, GitHub, and Slack workflows when connected, falling back to universal research/brief/deck workflows otherwise).

## Reason
Users taking over a remote VM for SSO or forms previously could not paste URLs or text into the remote desktop, had no way to stop a single runaway sub-agent without opening its task page, and saw static Home launchpads unrelated to their connected tools. Verified by `desktop/tests/tier2-phase1-check.mjs`.
