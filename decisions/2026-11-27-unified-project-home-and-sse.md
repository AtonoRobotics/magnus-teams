# 2026-11-27: Unified Project Home for Solo & Shared Projects and SSE Refresh (Phase 2)

## Choice
1. **One Project Home for solo and shared projects (`desktop/ui/projects.js`, `desktop/ui/teams.js`)**:
   - Bare `#/team/:ws` now resolves to `{ kind: "home" }` and renders `Projects.showShared(main, ws)` using the same `.project-page` layout as personal projects (`#/project/:id`): hero `@Magnus` composer, **Shared chats** (with inline `+ New chat`), **Shared documents** (with upload/download/delete), and **People** (with `Add or invite`).
   - Clicking a shared chat (`#/team/:ws/c/:ch`) opens the conversation, whose header breadcrumb links directly back to `#/team/:ws` (the Shared Project Home) instead of a side panel.
   - `#/teams` delegates to `Projects.showList(main)` so there is one unified Projects list page.
   - Trimmed `SharedCanvas.dockItems` inside shared conversations to **Files** and **Project settings** (legacy deep links still resolve).
2. **SSE-driven refresh for shared projects (`desktop/ui/app.js`, `desktop/ui/teams.js`)**:
   - Fixed `refreshDomains` in `app.js` where `Server.workspaces.list()` (which returns an array) was read as `w.workspaces || []`, and wired `Team.onWorkspaceEvent()` so SSE `workspaces` invalidations refresh the active shared project/chat immediately.
   - Replaced the unconditional 3-second polling loop (`TEAM_REFRESH_MS = 3000`) with a 30-second fallback (`TEAM_FALLBACK_POLL_MS = 30000`).

## Reason
Shared projects should feel identical to private projects plus shared documents and shared chats—opening a shared project should land on its Project Home rather than dropping straight into a Slack channel, and real-time updates should ride the existing `workspaces` SSE invalidation stream instead of polling every 3 seconds.
