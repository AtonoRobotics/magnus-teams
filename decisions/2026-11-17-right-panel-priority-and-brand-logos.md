# Right Side Panel Layout, Brand SVG Logos, and CSS Polish

## Choice
- Updated **only the right side panel (`#detail`)** in `desktop/ui/app.js`, `desktop/ui/app-logos.js`, `desktop/ui/layout.css`, and `desktop/ui/pages.css` while leaving the rest of the 4 themes (`obsidian`, `obsidian-2`, `prism`, `studio`) untouched:
  - **Compact Single-Row Header (`companionHeader()`)**: Replaced the tall centered avatar stack with a single horizontal header row showing the status-badged avatar, `Magnus`, live status pill, active theme link, task count, admin kill switch, and close button, followed by the rounded 4-tab segmented bar (`.panel-tabs`).
  - **Activity Tab (`activityView()`)**: Placed the inline **Needs your approval** banner (`.companion-needs-banner`) at the top with brand SVG logos, followed by the **Active now** progress card (`.companion-active-card` with `.active-progress`), **Recent work** (`timelineGroups()` with `24px` rounded `.tl-brand` logo tiles), **Up next** scheduled tasks, and a compact single-line **Cloud Computer** status bar (`.companion-computer`) at the bottom.
  - **Approvals, Files & Apps Tabs (`needsView()`, `filesView()`, `appsPanelView()`)**: Added `AppLogos.forText(text)` and the `linear` brand SVG in `desktop/ui/app-logos.js` so open and decided approvals, timeline tasks, and schedules render official multi-color brand SVGs (`Gmail`, `Google Calendar`, `Google Drive`, `GitHub`, `Slack`, `Linear`, `Outlook`, etc.), and added right-aligned monospace file sizes (`.lib-size`) in `filesView()`.

## Reason
The owner approved the compact, action-prioritized right side panel design with borderless matte cards and authentic SVG brand logos and requested applying it strictly to the right side panel without altering the rest of the themes.
