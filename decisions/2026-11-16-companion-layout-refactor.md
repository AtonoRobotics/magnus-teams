# Worker Companion Layout Refactor & Visual Theme Separation

## Context
After `9a8b690` introduced token-driven theme switching and initial companion panel helpers, testers reported that the initial landing layout still looked identical to the prior 2-column UI: the right Companion panel opened collapsed on desktop arrival, the sidebar lacked chronological grouping and live status indicators, cards inside the right inspector used legacy flat boxes rather than Grobot/Muse grouped `.gbox` sections, and switching between `Antigravity` and `Companion` changed radii/accents without strong structural differences in message bubbles, avatars, tabs, and elevation.

## Decision
- Auto-open the Worker Companion panel (`activity` or `needs` when waiting decisions exist) on wide desktop viewports (`innerWidth >= 1200`) in `home()` unless explicitly closed (`Panel.userClosed`), and auto-close an unpinned auto-opened panel below `1200px` in `Workspace.update()` so narrow viewports keep the conversation unobstructed.
- Refactor `companionHeader()` into the centered Grobot/Muse Worker Identity block (theme badge & close action top bar, centered identity emblem with corner edit trigger, status dot + `Worker Companion` subtitle, followed by flush underline tabs in Antigravity or a floating capsule track in Companion).
- Restructure `activityView()`, `timelineGroups()`, `needsView()`, `appsPanelView()`, and `filesView()` around grouped `.gbox` cards, a miniature Cloud Computer status screen graphic, open `Scheduled tasks` rows, collapsible day-grouped `Recent work` timeline stacks, badged `Approvals history` rows with indented details, `.companion-apps-hero`, and typed `.lib-badge` file lists.
- Group loose sidebar conversations into `Today` and `Previous` with live `.nav-dot` run/warn indicators and an Appearance shortcut next to Settings; show an inline `Project / Title` breadcrumb and live Companion toggle pill in `convHead()`.
- Differentiate `Antigravity` (full-width boxed editorial user prompt `.msg.me`, flush underline panel tabs, square monochrome squircle avatar, crisp hairline surfaces) from `Companion` (right-aligned rounded `.msg.me` bubble, circular gradient-tinted `62px` avatar, elevated `.say` composer & `.gbox` cards with `--shadow`, capsule `999px` tabs and primary buttons).

## Reason
Making the 3-column Worker Companion visible out of the box on desktop while cleanly separating the sharp monochrome workspace of Antigravity from the soft card-based Companion aesthetic turns the approved design system into an immediate structural upgrade without losing viewport responsiveness or contrast compliance.
