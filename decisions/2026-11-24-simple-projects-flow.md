# Simplify projects: quiet creation, unified full-stage view, direct deletion

**Date:** 2026-11-24

## Decision
Redesign the project creation, navigation, task-triggering, and deletion flows across the kernel and desktop UI:

1. **Magnus only starts a task when explicitly asked**
   - In `crates/magnus/src/team/conversation.rs`, `addressed()` now triggers only on explicit `@Magnus` mentions (`Why::Mentioned`) and no longer fires `Why::Named` on bare occurrences of the word `"Magnus"`.
   - In `crates/magnus/src/team/routes.rs`, `POST /v1/workspaces/{ws}/channels/{channel}/messages` accepts `no_agent: bool` so clean-room sharing (`SharedCanvas.openShareDialog`) and seeding notes into a new project never dispatch an agent run.
   - Creating or opening a project no longer auto-opens the `shared:missions` ("Start shared work") panel (`teams.js`, `team-command.js`).

2. **One calm, full-stage Project flow (no inspector/tab bouncing)**
   - Add `desktop/ui/projects.js` (registered in `desktop/ui/index.html` and `crates/magnus/src/ui/assets.rs`) to render `#/projects` (unified list of personal and shared projects) and `#/project/:id` directly in `#main` instead of inside the narrow right inspector panel under an unrelated `Activity` tab.
   - Replace the dual `Shared` / `All` links in the sidebar's `Projects` group (`desktop/ui/app.js`) with a single `+` button that opens a quiet inline name input (`Enter` creates and opens the project; `Esc` cancels).
   - Keep `Instructions` collapsed when empty and omit empty `Tasks` / `Routines` sections on fresh projects.

3. **Direct, low-ceremony deletion everywhere**
   - Add `DELETE /v1/workspaces/{ws}/projects/{id}` in `crates/magnus/src/team/kb_routes.rs` (emitting the existing `Change::ProjectRemoved` event) and `Server.workspaces.removeProject` in `desktop/ui/server.js`.
   - Expose direct Delete buttons on project rows in the sidebar, `#/projects`, `#/teams`, the full-stage Project header, and the shared conversation header (for creators).
   - Drop the typed-name gate (`askTyped`) for solo shared projects (`members.length <= 1`), keeping it only when deleting a shared project that has other collaborators.

## Reason
Previously, projects were split across three competing concepts (personal projects in the right inspector panel, shared projects on `#/teams`, and workspace sub-projects via `⌘K`), creating or sharing a project could accidentally start a Magnus task, and deleting projects was either impossible (workspace sub-projects) or buried behind `Members` → `Danger zone` → exact-name typing. This makes projects simple, quiet, and direct with zero ceremony.
