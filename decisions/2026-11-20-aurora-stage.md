# Aurora's conversation and cards work as the mockup's agent demo

## Choice

In Aurora and Obsidian 2 Aurora:

- The conversation is the main card. Under it, in its column, is a row of
  cards, one for each companion view with the panel's icons: Live Workspace,
  Approvals, Up Next, Recent Work, Rules, Files, Apps and the computer. Each
  opens what it shows.
- The conversation drives the cards. Work it starts lights Live Workspace
  with the execution beam and what the work is doing now; a decision waiting
  turns Approvals amber with what waits; finished work turns Live Workspace
  mint with its result. The cards move to put what needs the person first,
  sliding to their new places.
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
layout (cards under the conversation, not beside it) and asked that the
interplay of the conversation and the cards follow the mockup's agent demo.

## Reason

The mockup's `runWorkflow` shows one stage: the prompt's answer is the main
box (reasoning, streamed words, "Inspect Canvas"), the small boxes beside it
in the same column go executing then done as the work moves, and the result
slides open on the right while the column narrows. Every companion view must
remain with its icon, which the mockup does not show, so each became a card.

## Verification

`desktop/tests/aurora-stage.mjs`, run by the `appearance` scratch test.
