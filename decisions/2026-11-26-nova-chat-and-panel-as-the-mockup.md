# Nova's conversation and right panel are drawn as the mockup's stage

## Decision
In Nova, the conversation and the panel beside it follow the stage in
`docs/nova-greenfield-mockup.html`:

- **One canvas.** The panel has no surface of its own. It sits on the
  stage's flat carbon (white in light mode), parted from the conversation by
  one hairline, and the head's line runs across both. The splitter keeps its
  width so it can still be dragged.
- **No aurora wash.** Nova's canvas shows only the drifting points. The
  aurora glow behind them is hidden.
- **Panel width.** The open panel takes about 46% of the stage (at least
  340px, at most 640px), as the mockup's 54 / 46 split does. Its content has
  20px of padding.
- **Approval queue.** The Approvals tab opens on the mockup's head:
  "Approval queue", an amber count of what waits, and the A / D keys at the
  right. With nothing waiting, it says "0 waiting".
- **Magnus signs its latest answer** with its 26px face, its name in 12px
  semibold, then the model and time in 11px mono. Older answers give up
  their face.
- **The person's bubble** has no rim, at 14.5px.
- **At rest, nothing over the composer.** "Ready · standing by" shows no
  more. The line still shows while Magnus thinks, works, waits or has just
  said something.

The tabs stay the six that spec §4.4 names (Activity, Tasks, Routines,
Approvals, Files, Apps), with the pill Close it asks for. The mockup shows
three tabs; keeping six loses nothing the spec promises.

## Reason
On 2026-11-26 the owner said the right panel and the chat did not match the
mockup. Measured against it:
- The panel was a lighter grey slab (#141517 against #0B0C0E), with a 16px
  strip and two lines between it and the conversation.
- The stage carried a blue wash.
- The panel was 457 of 1284px wide, where the mockup has 46%.
- The queue had no head.
- The answer's byline was one muted line, with no face or name.
- The bubble had a rim.

`desktop/tests/nova-screens.mjs` checks each point in the real app, in both
modes.
