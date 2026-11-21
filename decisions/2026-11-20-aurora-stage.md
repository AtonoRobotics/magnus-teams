# Aurora's conversation and cards work as the mockup's agent demo

## Choice

In Aurora and Obsidian 2 Aurora:

- The conversation is the main card. Under it is the bento, two columns as
  in the mockup: a box appears while something is relevant and leaves when it
  stops, and the boxes already there slide to their new places. Live
  Workspace while work in this conversation runs (with the beam and what it is
  doing), waits on the person (amber) or has just finished (mint, with its
  result); Approvals while a decision waits; Up Next while a routine is due
  within a day; Files while a file is new; Apps while one needs attention;
  the computer while it works or asks for the person; Recent Work while work
  in another conversation has just finished. What needs the person comes
  first; a lone box, or the most urgent of three, spans the row. With nothing
  relevant the bento folds away.
- Every view stays one click away: the conversation's head carries the
  panel's icons for Approvals (with its count), Tasks, Routines, Rules, Files
  and Apps.
- There is no panel beside the conversation. The overview is the cards.
  Anything opened (a task, a file, a decision, the full view behind a card)
  is the artifact canvas: it slides in on the right and the conversation and
  its cards narrow together, sharing the stage about evenly (the mockup's
  1.02fr / .98fr). When work the person is watching finishes or needs them,
  its result slides open on its own, unless they opened something else.
  Close slides it out and the conversation widens again.
- A new block of an answer streams in word by word.
- Other styles keep the panel as it was.

This supersedes `2026-11-20-aurora-bento.md`. The owner chose the mockup's
layout (cards under the conversation, not beside it), asked that the
interplay of the conversation and the cards follow the mockup's agent demo,
and then that the boxes come and go as they are relevant instead of standing
in a constant row.

## Reason

The mockup's `runWorkflow` shows one stage: the prompt's answer is the main
box (reasoning, streamed words, "Inspect Canvas"), the small boxes beside it
in the same column go executing then done as the work moves, and the result
slides open on the right while the column narrows. Every companion view must
remain with its icon, which the mockup does not show, so each became a card.

## Verification

`desktop/tests/aurora-stage.mjs`, run by the `appearance` scratch test.
