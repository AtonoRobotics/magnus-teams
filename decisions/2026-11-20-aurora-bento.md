# Aurora shows the workspace as a bento, not a panel beside the conversation

## Choice
In Aurora and Obsidian 2 Aurora (`Theme.bento`):
- The companion panel no longer opens on its own beside the conversation.
  `app.js` asks `companionBeside()` before each automatic open; every other
  style opens it as before.
- Under the conversation, `bento.js` draws four tiles from what the app
  already holds, as the mockup's bento draws its mini tiles:
  - Live Workspace: the running task and its status line, or the connected
    apps when nothing runs;
  - Human Control: the decisions waiting on the person, or that Magnus asks
    before external actions and payments;
  - Up Next: the next schedule;
  - Recent Work: the last finished task.
  Each tile has a status dot: cyan while working, amber when it needs the
  person, mint when done, red when stopped, grey when idle.
- A tile opens its view with `data-open`, so the panel slides in as the right
  column only when the person opens something (the stage slide of
  `2026-11-20-aurora-stage-slides.md`). A panel the person opened stays open.
- The conversation is the main glass card and wears the beam ring while work
  runs. When the column is narrow (the panel open on a small window, or a
  phone), the tiles are one row that scrolls sideways, without their
  descriptions.
- Work waiting on the person is a state of its own: the state pill reads
  "Needs you" and the ambient light turns amber, instead of "Ready" or
  "Working".
- The request card's head is the mockup's reasoning pill, with a spark that is
  amber when a task needs the person, cyan while one works and mint when all
  are done. Its tasks are tiles with their icon in a tinted status ring.
- An approval reads as the mockup's artifact panel: a ruled head, the
  consequence and "See exactly what it would do" as inner cards, and its
  answers in a ruled foot as pills. Buttons and chips are pills throughout.
- Primary buttons keep the person's accent. With the default accent they are
  the mockup's white pill; another accent is a choice the person made.
- No `backdrop-filter`: the app forbids it everywhere
  (`desktop/tests/companion-panel-check.mjs`). The glass comes from
  translucent fills over the ambient canvas.
- The widgets live in a new sheet, `aurora-widgets.css`. `aurora.css` keeps
  the frame, the canvas and the motion. Both stay under 800 lines.

## Reason
The owner: "There is suppose to be a bento box instead of right side panel",
and the widgets should "render like the mockup". The mockup's stage is a
bento (one main card and mini tiles with status dots). Its artifact column
slides in only when a deliverable is opened. Aurora instead opened the full
companion panel on every wide window.
`desktop/tests/aurora-bento.mjs` (run by the scratch suite's `appearance`
test) checks:
- the tiles, their tones and where they open;
- that the automatic panel closes;
- the layout, including the narrow row;
- that the tiles are not redrawn for an unchanged workspace;
- the "Needs you" state and the spark colours;
- that no other style gets a bento.
