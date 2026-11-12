# The usage-data question: asked once, two equal answers, changed any time

The owner, 2026-11-11: "Create a popup with accept or deny to help improve
magnus by sharing anonymous usage data."

Choice:

- After the Terms of Use, each person is asked once, on their next sign-in
  after this ships: "Help improve Magnus", what is shared and what never is,
  and two buttons, "Share anonymous usage data" and "Don't share". The buttons
  are the same element with the same style; neither is focused or chosen
  beforehand (the dialog's text takes the focus); nothing is pre-ticked.
  Escape puts the question off: nothing is recorded, nothing is shared, and
  it is asked again next time.
- The answer is `legal` event `usage_chosen` with `share` and the version of
  the question's text (its date and a digest of the text and both labels).
  An answer to an older text counts as unanswered, so changing what the
  question says asks again.
- Yes: their work appears in developer views in the anonymous form only. No,
  or no answer: nothing of theirs reaches developers, not even anonymised,
  and their runs are left out of every count (only the capture-overhead
  panel, which holds no per-person data, counts the whole server).
- Settings › Agreements shows the same question with the current answer marked
  (`aria-pressed`) and both buttons; a change applies from then on.

Why unanswered counts as no: sharing is a promise about their data that they
have not made.
