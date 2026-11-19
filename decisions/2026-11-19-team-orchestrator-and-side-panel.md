# Team agent orchestrates personal agents for connector use, and the team right side panel stays docked on the right

## Choice
1. **Team agent delegates personal connector requests via `team_ask_personal_agent` (`crates/magnus/src/team/delegate.rs`)**:
   - When a team member asks the team agent to read or act in a personal/OAuth connector (such as Gmail, Google Calendar, GitHub, Slack, or Drive), the team agent calls `team_ask_personal_agent(request, app, member)` instead of calling personal OAuth connectors directly with a shared workspace credential.
   - If the request targets the requester's own personal agent, the team agent runs a private whisper task under the requester's identity and returns the result as a private whisper (`TeamWhisper`) so nothing from a personal connector enters the shared channel until the member chooses **Share in channel**.
   - If the request targets another workspace member's personal agent (or an app only another member has connected), the team agent sends a private whisper in the channel to that member (`Your Magnus · only you see this`) and starts their personal agent asynchronously so their data is never returned into the requester's run without their consent.
   - Non-OAuth workspace-scoped connectors remain callable by the team agent so existing team connector policies and invariant suites continue to work unchanged.
2. **Team right side panel layout and resizer (`desktop/ui/teams.css`, `desktop/ui/teams.js`, `desktop/ui/team-panel.js`)**:
   - Removed the `@media (max-width: 1200px)` rule in `desktop/ui/teams.css` that switched `.team-shell` to a 2-column grid at the default 1200px Tauri window width while `.team-side:has(> .team-companion)` overrode `.team-side { display: none; }` with higher CSS specificity `(0, 2, 0)` and wrapped `#team-side` into Row 2, Column 1 on the left side under `#team-rail`.
   - Added `#team-splitter` between `#team-center` and `#team-side` (`grid-template-columns: 220px minmax(0, 1fr) 6px min(var(--panel-width, 340px), 36vw)`), wired to `Theme.prefs().panelWidthPx`, and kept `#team-side` populated across all Team views (`channel`, `project`, `activity`, `people`, `search`).
   - Added **Personal agent delegations** to the Activity tab and **Connectors & agents** to the Members tab of the right side panel.
3. **Review bug fixes and safety controls**:
   - Fixed `askText` options-object handling (`{ title, body, placeholder, okLabel, danger }`) and `ask` danger styling in `desktop/ui/app.js`.
   - Fixed in-flight task steering notice step indexing (`run.replies.len() as u32`) in `crates/magnus/src/api/work.rs` and `crates/magnus/src/chat.rs`, plus trailing notice fallback in `crates/magnus/src/run/mod.rs`.
   - Added startup validation for `model.fallbacks`, model access checks during failover, and `input_tokens` re-estimation in `crates/magnus/src/config/load.rs` and `crates/magnus/src/run/agent_loop.rs`.
   - Cleaned up sole-member workspaces (`delete_workspace`) on self-service and admin account deletion.
   - Added `POST /v1/tasks/stop-all` ("Stop my active work") and Working Hours / Quiet Hours / Daily Interruption Limit controls in Settings › Notifications.

## Reason
- Personal OAuth connectors carry an individual's credentials and private data. Having the team agent act as an orchestrator that messages each member's personal agent preserves the privacy boundary (Invariant 3 / Whisper privacy) while letting the team coordinate across everyone's connected apps.
- At the default 1200px window width, the specificity mismatch between `.team-side` and `.team-side:has(> .team-companion)` caused CSS Grid auto-placement to push the right side panel into the left rail column. Removing the 1200px breakpoint and moving `display: flex` onto `.team-side` keeps the 3-pane layout intact and resizable down to 860px.
