# Aurora says what Magnus is doing where the person is looking

## Choice

In Aurora and Obsidian 2 Aurora (`aurora-reasoning.js`, styles in
`aurora-widgets.css`):

- An answer being written shows the mockup's reasoning pill in place of the
  still "Working on an answer…". The pill reads the answer's task
  (`GET /v1/tasks/{id}`) every 1.5 s (`REASON_POLL_MS`) while it runs. Its
  words are the step the task is on. Its steps are the plan's units when the
  task has a plan; otherwise each new status line becomes a step and the one
  before it ticks off. It is amber when the task waits on the person.
- When the answer comes, the same pill sits above it, turns mint, says
  "Completed N steps · Xs" and folds. A click opens it again. Answers that
  finished before this session have no pill.
- The pill is one node per answer, kept across the thread's repaints and
  changed in place, so its changes play as transitions instead of being drawn
  finished. A re-attached pill takes its old style before it changes.
- Aurora's state engine counts the pill: an answer being thought out or
  written is Thinking, a step being worked is Working, a wait is Needs you.
  Before this, a plain chat answer left Aurora at Ready.
- Above the composer, a line gives the state in its colour, as the mockup's
  presence label does ("Ready · standing by", "Working across your apps…").
- The composer glows on focus, and grows and shrinks over 180 ms as the
  person types: one change per frame, from the height drawn to the height
  the composer set.
- Other styles are unchanged.

## Reason

The owner said the chat box "doesn't match the mockup's motion and status
updates". The mockup's main card says what Magnus is doing step by step and
folds when the answer comes. The app showed one grey line until the answer
replaced it, and Aurora's state never left Ready during a plain answer. The
running task already carries its plan and a plain-words status line
(`evidence::status_line`), so no server change was needed.

The owner asked to see a filmstrip before this ships:
`desktop/tests/aurora-filmstrip.mjs` records the mockup and the app playing
the same request, frame by frame and as video.

## Verification

`desktop/tests/aurora-reasoning.mjs`, run by the `appearance` scratch test.
