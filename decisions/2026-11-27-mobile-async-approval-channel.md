# 2026-11-27 — Out-of-Band Mobile / Async Approval Channel (#/approve + PWA)

## Choice
- Added a dedicated mobile-first approval triage route `#/approve` (and `#/approve?ask=<id>`) in [`desktop/ui/mobile-approve.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/mobile-approve.js), registered in `app.js`, `index.html`, and `crates/magnus/src/ui/assets.rs`.
- Reused `askCard(a, true)` from `asks.js` verbatim so server-authored destination previews, risk tier badges, and Edit-before-allowing preserve all prompt-injection invariants on mobile.
- Added a batch `"Allow all N reversible"` button that strictly excludes `irreversible` and `financial` asks.
- Added `desktop/ui/manifest.webmanifest` (`start_url: "/#/approve"`, `display: "standalone"`) and a `"Copy mobile link"` card in `Settings → Notifications` so users can add Mobile Approvals to their phone's home screen with zero new auth plumbing.

## Reason
- Autonomous workers frequently pause on Tier 1–3 approvals when the user is away from their desktop.
- Leveraging Magnus's existing server-served web UI and OIDC/passkey auth makes mobile approval triage immediately usable without introducing a new credential surface.
