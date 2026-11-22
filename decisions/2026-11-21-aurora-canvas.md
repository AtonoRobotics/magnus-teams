# A decision on the canvas is built around the decision

## Choice

Step E of the Aurora composition (`2026-11-21-aurora-composition.md`). When
the canvas shows a decision still waiting on the person
(`AuroraStage.shell` in `desktop/ui/aurora-stage.js`):

- **The question is the title.** The ask's one sentence is the canvas's
  heading and its accessible name, in place of "Item". What kind of decision
  it is ("Approval · Cannot be undone") stands small above it.
- **No Back, only Close.** The canvas is opened for this one decision; Back to
  the list of what waits was a second way out that led somewhere the person
  had not asked to go. Close shuts the canvas, as it does everywhere.
- **What it would mean is under the title** (the consequence the server
  words), seen at once.
- **The rest is behind the one disclosure** that shows exactly what it would
  do: the app it uses, who waits on it, when it was asked, the task it is
  for. Shut at first; one click opens it. A decision with no content gets the
  disclosure only if there is something to put behind it.
- **Its answers are at the canvas's foot**, and stay there in reach as the
  details are opened and scrolled.
- **The card stays the decision's card.** It is restyled flat, not a card in
  a card, and keeps its answers and, unseen, its question, so what reads the
  card (aurora-alerts.js wording the decision, closing the canvas once it is
  answered) still finds them.

- **Answered there, it keeps its question.** The canvas says the answer at
  its foot, where the answers were, under the same title, and then slides
  away as before (aurora-alerts.js).

A decision answered elsewhere, and everything else the canvas shows, are
drawn as before.

## Reason

The composition plan's step E: "the canvas built around the decision (title
and Close, the result, the main action at the foot, details behind one
disclosure)". The owner's rule for the whole interface: "What is important
slides into view. When it doesn't need user focus it disappears." On the
canvas what is important is the question and its answers; the app, the
timing and the task are there for whoever wants to check them.

## Verification

`desktop/tests/aurora-canvas.mjs` (the `appearance` scratch test): the
question as title and name, the kind above it, Close and no Back, one
disclosure holding the app, who waits, when and the task, shut at first, the
consequence seen, the card keeping its question and answers, the answers at
the canvas's foot with short and with long details open, answering it, and an
answered decision and a task drawn as before.
