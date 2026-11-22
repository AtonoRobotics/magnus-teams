# Aurora's decisions and alerts are part of the stage, not over it

## Choice

In Aurora and Obsidian 2 Aurora, everything that wants the person is placed
on the stage, the one composition of the conversation, its bento and the
canvas beside it. Nothing floats over it, and nothing goes on a timer while
it still needs the person (`aurora-stage.js`, `aurora-alerts.js`,
`aurora-reasoning.js`; styles in `aurora-stage.css` and `aurora-widgets.css`):

- **A decision about the conversation on screen** opens the canvas beside
  it, with what it would do and its answers. This is the mockup's "Approve &
  Send Drafts" panel. It opens on its own unless the person opened
  something else themselves, and each decision opens once. Answered, the
  card says the answer where it was ("✓ Allowed · Magnus has your answer."),
  then after 900 ms the canvas slides away. The next decision of the
  conversation, if any, slides in.
- **The thread keeps one quiet line** for that decision in place of the
  card: "Needs your decision · Book the Lisbon flight for $412? · Review",
  then "✓ Allowed" once answered. A decision is no longer drawn twice in the
  same window.
- **A decision from elsewhere** is a box in the bento, with its answers in
  the box when it is an ask that takes one tap. Otherwise its title opens it
  in the canvas. Answered, the box says the answer, then leaves, and the
  others slide into its room. The decision in the canvas is not also a box.
- **A teammate's message** that would be a pop-up is a Messages box in the
  bento instead. It stays until the person opens it or visits where it was
  written, or for 30 minutes. The system notification still comes when the
  app is in the background. An "inbox" pop-up is dropped: the stage already
  shows that decision.
- **What a toast would say** ("Allowed.", "Saved.") is said in the line
  above the composer for the toast's time, as a status for screen readers.
- **Where there is no stage** (no conversation on screen, such as Settings),
  a message is a pop-up and a toast is a toast, as in the other styles, so
  nothing is missed.
- In a list of decisions (Approvals), one arriving rises in, and one
  answered says the answer, then folds away.
- Reduced motion puts things in the same places without the movement.
  Other styles are unchanged.

`app.js` and `notices.js` each gain one line that hands the toast or the
pop-up to Aurora when it is on. The cards, the answers and how a decision is
sent are unchanged.

## Reason

The owner set the concept: "make the UI automated so the user doesn't need
to learn it. What is important slides into view. When it doesn't need user
focus it disappears. Everything lays out in the same window in the best
composition for the user to make decisions fast and with the least amount
of searching." Before this, a decision showed in the thread, in the bento
and in the Approvals list at once. Pop-ups floated over the stage and left
after 8 s whether or not they were dealt with, and toasts repeated what had
just happened. A first version that animated the pop-ups and thread cards
was set aside before shipping, because it polished the layout the concept
replaces.

The owner asked to see a filmstrip before this ships:
`desktop/tests/aurora-filmstrip-alerts.mjs` films the mockup's approval
beside the app's, and the app's other alerts on its stage.

## Verification

`desktop/tests/aurora-alerts.mjs` (15 checks), run by the `appearance`
scratch test. It plays the page in `desktop/tests/aurora-alerts-page.mjs`,
which the filmstrip films too.
