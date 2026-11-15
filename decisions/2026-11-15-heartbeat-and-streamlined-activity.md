# Direct Schedule Creation, Heartbeat Preset, and Streamlined Activity / Needs You

## Decision
1. Exposed `Server.schedules.create` (`POST /v1/schedules`) in `desktop/ui/server.js` and wired the Automation Discovery Wizard's "Start Selected" action in `desktop/ui/onboarding.js` to create real recurring schedules directly (and start chat turns for on-demand workflows).
2. Added a 1-click **Heartbeat (30m)** toggle card in `schedulesView()` (`desktop/ui/app.js`) using the kernel's `every_minutes: 30` cadence.
3. Streamlined `activityView()` to prioritize **Active & Needs Attention** + **Schedules** and collapse **Completed work** into a closed `<details>` summary, and simplified `needsView()` to show a single *"You're all caught up"* empty state when no decisions are pending.

## Reason
`POST /v1/schedules` already existed in the kernel (`crates/magnus/src/api.rs`), so exposing it on the client turns Discovery Wizard selections and the Heartbeat check-in into real background schedules immediately. Collapsing completed tasks in Overview and removing double empty states in Needs you keeps active work and approvals front-and-center.
