# Aurora's workspace slides open and closed, as the mockup's stage does

## Choice
In Aurora and Obsidian 2 Aurora, on desktop widths:
- The shell keeps four columns whether or not work is open: navigation,
  conversation, splitter and panel, with the splitter and panel at `0px` when
  closed. Columns and the top bar's row transition over `--aurora-stage-ms`
  (520 ms, the mockup's stage takes .55 s) when motion is Full.
- The panel keeps its own width (`min-width: var(--panel-width)`) while its
  column grows, so it slides in from the right edge rather than reflowing
  from nothing; it fades in with the existing `.opening` glide.
- On close, `aurora.js` keeps the panel's last frame (the nodes `closePanel`
  removed, frames replaced by their space, ids dropped) where the panel was,
  and slides it out while the column folds; it is removed when its animation
  ends.
- No transition while the splitter is dragged or the window is resized, and a
  conversation read at its end stays at its end while its column changes width.
- `app.js` is unchanged: the panel still opens, closes and empties at once,
  and every other theme keeps its layout.

## Reason
The owner asked for Aurora's layout to "slide and change like the mockup";
the panel appeared and vanished in one frame. The shell went from two
columns to four when work opened, and the browser only interpolates grid
tracks between lists of the same length, so the existing transition never
ran. `desktop/tests/aurora-motion.mjs` checks the columns grow and fold over
time, the panel keeps its width mid-slide, and the closing frame slides out
and is removed; those fail on the previous stylesheet.
