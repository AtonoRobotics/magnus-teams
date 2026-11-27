# Nova has one top bar across every screen

## Decision
In Nova, a bar runs across the top of every screen, beside the rail and over
the stage and the right panel, as `docs/nova-greenfield-mockup.html` draws it
(`desktop/ui/nova-frame.js`).

- **Left:** where the person is, in the mockup's words ("New conversation",
  "Project Hub · <project>", "Morning Briefing & Routines", "Explore &
  Artifacts", "Apps & Skills"), then this month's spend from `/v1/spend`.
  The mockup shows the day's spend against a daily cap. The server keeps
  only a monthly spend and limit, so the bar says "Spend this month", as the
  Briefing's spend card does.
- **Right:** the companion pill and Light / Dark (spec §4.2's quick
  controls). They were in the conversation's head, so pages other than a
  conversation had none.
- **Conversation head.** A conversation keeps its head under the bar, as in
  the mockup: the breadcrumb, •••, the bell and the sidebar toggle.
- **No close.** Nova pages no longer get Aurora's lone close at the top
  right. The rail is the way out, as in the mockup.
- **Narrow windows.** Where the window is narrow and the workspace bar
  switches between work and the conversation, that bar keeps the row.

The navigation and the right panel follow the mockup too:
- **Rail.** Order and icons follow the mockup:
  - From the top: the companion, New conversation (a pencil), Inbox (a
    tray), Projects, Briefing, Explore (a grid) and Apps (layers).
  - At the foot: the sidebar, search, and the person's initial, which opens
    Settings, where a boxed gear was.
- **Sidebar.** It opens beside the stage, as in the mockup, rather than
  over it behind a dimmed backdrop. Ctrl+B or the rail's button open it,
  and its header's collapse button closes it (the pinned sidebar
  preference).
  - Quick Jump (⌘K) sits at its foot, above the person, in place of the
    search field at its top.
  - A window too narrow to keep it beside the stage still slides it over.
- **Right panel.** The tabs are a segmented pill in one 52px row, level
  with the conversation's head, then Close. The tab in use names the panel,
  so the title row above the tabs is gone.
- **Pages.**
  - The Briefing, Apps and the list of projects have the mockup's quiet
    22px heads in a centred column.
  - The Briefing's figures are monospaced and its section label is small
    capitals.
  - On New Chat the head under the bar is gone, since the bar already says
    "New conversation".

Titles are shown as words. A conversation whose first message began with
Markdown was named "**Plan** the offsite"; it is now "Plan the offsite".

`desktop/tests/nova-screens.mjs` (scratch test `nova_screens`) opens each
screen in the real app in both modes, checks the bar, and saves the
screenshots to put beside the mockup's.

## Reason
On 2026-11-26 the owner said release after release changed nothing they
could see. Earlier rounds fixed details the spec names inside each screen,
and checked them only with code checks. The frame around every screen never
looked like the mockup. Spec §4.2 calls the conversation's head the top bar,
and that head is kept. The mockup draws a bar above it on every screen, and
the spec does not say otherwise, so Nova follows the mockup.
