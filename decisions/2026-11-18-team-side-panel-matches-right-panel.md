# The team side panel is laid out as a person's own right panel

## Choice
The panel beside a team channel or project (`desktop/ui/team-panel.js`) uses the right panel's layout and classes: one head row with the agent's initial and status dot, the project's or workspace's name, a status pill ("Working · N active", "N waiting on someone", "Ready") and "N open tasks · N members"; the four-tab segmented bar (Activity, Tasks, Files, Members); the open tab's sections; and the team computer at the foot ("Team Computer · Awake / Asleep / Being set up", Watch →).

- **Activity**: work waiting on a person as the banner at the top, the agent's current work as the "Active now" card (its step, its channel), the team computer's screen with Take control, any further work, and the month's spend.
- **Tasks**: progress and the open tasks, with brand logos where a title names a known app, and the way to the board.
- **Files**: newest first, who added each and its size.
- **Members**: the faces and the way to the people page.

The tabs are buttons kept in the panel (`TeamPanel.sideTab`), not links: a team's place is in the address, and the side panel's tab is not a place. `.panel-tabs` styles links and buttons alike.

## Reason
The owner asked for the team chat's panel to be updated the same way as the personal right panel (decisions/2026-11-17-right-panel-priority-and-brand-logos.md). Only real numbers are shown: the personal panel's placeholder progress width and its fallback app count were not carried over.
