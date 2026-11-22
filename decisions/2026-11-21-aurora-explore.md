# Aurora's Explore is the mockup's bento

## Choice

In Aurora and Obsidian 2 Aurora, Explore's playbooks are laid out as the
mockup's showcase (`website/`, `.visual-grid`). Styles are in
`aurora-stage.css` and motion in `aurora-explore.js`.

- **Layout:** the first playbook shown is a wide 21:9 card across the grid.
  The rest are 4:3 cards in pairs. On a phone there is one column and the
  first card is 4:3 too, as the mockup has it.
- **Each card:** the playbook's image fills the card under a dark fade, and
  zooms in a little (3% over 0.8 s) on hover. Its words sit at the foot over
  the fade, and the kind of playbook sits at the top.
- **A filter change moves:**
  - a playbook still shown slides from where it was to where it is, growing
    into the wide place when it becomes first;
  - one the filter takes away fades out where it was, out of reach of
    pointer and screen reader;
  - one coming back rises in, one after another.
  
  discover.js still redraws the page at once. The change is played from
  where things were, taken just before the redraw.
- **Reduced motion and other styles:** reduced motion keeps the bento
  without the movement. Other styles keep their grid of equal cards.

discover.js gains a `data-playbook` id on each card, so a playbook is known
across a redraw.

## Reason

The owner said "the explore tab is not bento box and does not have motion".
This is step 5 of the plan they accepted: Explore as the mockup's bento,
with the wide image card, then 4:3 pairs, the image zooming on hover, and
filter changes sliding. The mockup now has Obsidian colours (91971a2), and
the owner confirmed that is right. Only its shapes and motion are taken
here. The app keeps its own Aurora colours.

The owner asked to see a filmstrip before each step ships:
`desktop/tests/aurora-filmstrip-explore.mjs`.

## Verification

`desktop/tests/aurora-explore.mjs` (11 checks), run by the `appearance`
scratch test, on the page in `desktop/tests/aurora-explore-page.mjs`.
