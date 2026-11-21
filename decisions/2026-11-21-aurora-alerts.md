# Aurora's alerts move as the mockup's do

## Choice

In Aurora and Obsidian 2 Aurora (`aurora-alerts.js`, styles in
`aurora-widgets.css`), with full motion:

- A pop-up in the lower corner is glass, with a dot for what it is about
  (cyan for a message, amber for something that needs the person, violet for
  an update) and a line along its foot for the time it has left. The line
  holds while the pointer or focus is on the pop-up and starts again when
  they leave, as notices.js's own timer does.
- A pop-up or a toast that goes slides out where it was, and the ones beside
  it glide into the room it leaves (420 ms, `ALERT_LEAVE_MS`). Before this,
  both happened in one frame.
- A decision that arrives while the person is on the page rises in. One that
  was already there when the page was drawn, or when the person changed
  page, does not.
- An answered decision shows the answer where it was, as the mockup's
  approve button turns to "✓ Sent & verified": a mint "✓ Allowed" (or
  Declined, Sent, Kept, …) with "Magnus has your answer." It holds for
  900 ms (`ALERT_CONFIRM_MS`), then folds away. Before this, the card
  disappeared with the next repaint.
- notices.js, app.js and asks.js are unchanged. They still remove and redraw
  at once. What they removed is played out by a copy that nobody can reach
  (inert, hidden from screen readers, without ids), so focus, ids and the
  real page are what they would be without Aurora.
- With reduced motion, and in other styles, everything goes at once as
  before.

## Reason

The owner said the alerts "are still largely static sized and don't match
the mockup's motion and status updates". The mockup confirms an approval in
place and keeps it in view. The app dropped the card, and its pop-ups and
toasts vanished in one frame and moved the rest in the same frame. Playing
the removal out afterwards keeps the existing drawing code, so nothing about
what a decision does or when it goes changes.

The owner asked to see a filmstrip before this ships:
`desktop/tests/aurora-filmstrip-alerts.mjs` records the mockup's approval
beside the app's, and the app's pop-ups, which the mockup does not have. The
recording and the side-by-side sheet are shared with the chat filmstrip in
`desktop/tests/filmstrip-kit.mjs`.

## Verification

`desktop/tests/aurora-alerts.mjs`, run by the `appearance` scratch test.
