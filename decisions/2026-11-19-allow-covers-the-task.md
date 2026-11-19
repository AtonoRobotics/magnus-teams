# Allow covers the task, in every app; hard to undo is no ask of its own

The owner reported that Magnus asked for approval on nearly every change of
a task they had already given and approved: GitHub was the example, and the
cause was general. The fix had to be too: no app gets an exception.

**Allow was matched per tool.** An ask's Allow ("covers the rest of the
task") was matched on the same tool, the same risk class and exactly the
same destinations. A task that commits, then opens a pull request, then
comments met three tools, so it asked three times, and the same held for a
deploy then a domain change, or a task update then a comment, in any app.
"Allow always" is offered only where an ask names a destination, so for most
apps, which name none, there was no way to stop it at all.

Allow ("task" scope) now covers every later action of its task, of any tool
in any app, whose destinations are all ones the person allowed. An action
that names no destination adds none and is covered; a recipient the Allow
did not name is not (a Bcc injected into a draft still asks). What does not
change:
- a payment asks every time, and is covered only by its own approval;
- case 3 (private data to a new destination after untrusted input,
  invariant 4) is covered only by an Allow given on a case-3 card, for the
  destinations it named;
- an Allow holds for its task (and that task's sub-agents and chat turns)
  only, and only when the person whose work it is gave it;
- the person's own block and ask-first rules, prohibited tools, the hard
  denials and the organisation's policy denials come before any Allow;
- "once" approvals, and window and standing approvals from before rev 140,
  still cover only the action they were given for.

**Auto Review asked about everything hard to undo.** Its instructions said
"Ask when you are unsure, and when the action would be hard to undo", and
apps class most writes `irreversible` (a commit, a comment, a deploy). So
the first action of each kind asked even when it was exactly what was asked
for. The spec's case 1 is an action Auto Review judges *outside what was
asked*; irreversibility alone is not one of the four asks. It now allows an
action that does what the person asked even when hard to undo, and asks
when unsure whether it serves the request or when a hard-to-undo action
goes beyond it.

An earlier attempt on this branch made a GitHub repository a destination
(`place`). The owner rejected it as a per-app exception, and it was removed.

Tests: `asks::tests::allow_covers_the_task_wherever_it_goes_nowhere_new`,
`asks::tests::an_old_window_or_standing_approval_covers_only_its_own_tool`,
`auto_review::tests::what_cannot_be_undone_asks_only_beyond_the_request`.
