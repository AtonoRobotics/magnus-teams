# Aurora's motion plays for what arrives, never for a repaint

## Choice
- `aurora.js` keys every entrance on what is shown, not on the DOM node: a
  view enters when the route changes, the panel when its `data-panel`
  changes, the new-conversation hero once per visit, and an answer's block
  when that block is new. The per-entry stagger on `.thread > .entry` is
  gone; the app's own `.entry.enter` already animates a new message.
- Entrances are marked inside the MutationObserver callback, before the
  browser draws the new nodes, not after a 45 ms timer.
- The hero no longer resets the conversation's scroll.
- Focusing the composer is no longer Aurora's "thinking" state.
- The execution beam is a ring drawn over a card's edge (`::before`, masked);
  the card keeps its own surface when work starts or stops.
- No CSS `filter: blur()` on the ambient canvas or in entrance keyframes. The
  canvas's radial gradients are already soft; the blur was recomputed over
  the whole window every frame.

## Reason
The owner saw Aurora "flash randomly and refresh the window". `app.js`
`paint` replaces a region's markup whenever it changes, so every live update
makes new nodes. Aurora treated each new node as an arrival: one repaint of
an unchanged conversation and panel replayed five entrance animations, each
first drawn, then hidden, then faded back in 45 ms later. A repaint of the
thread also reset its scroll to the top, and clicking into the composer
swung the whole background to the "thinking" palette.
`desktop/tests/aurora-motion-check.mjs` (run by the scratch suite's
`appearance` test) fails on each of these with the previous code.
