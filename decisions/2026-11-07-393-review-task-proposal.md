# Reviews propose tasks to the inbox (#393)

#254 gave chat turns `propose_task`: a grounded, budgeted card in the
thread. #382 then ruled reviews out of it ("you cannot propose task
cards outside chat"), so review findings that needed work went out as
observations and the person retyped them as tasks. The owner (on #393)
ruled the other way: review matter must become a real task proposal
with what was noticed, what's needed, and an approval that starts it.

A review has no thread, so its proposals land in the inbox as a new
`task_proposal` kind rather than thread cards. Everything else is the
#254 contract unchanged: the same validation (title, text, noticed,
outcome, why now, permissions, a cost within the appliance's limit),
the same evidence rule (each item a call read this turn or a task of
the person's, resolved or refused), and the started task spending at
most the approved estimate. Accept starts the task with the card's
budget, scope and title under the proposing run's model and taint;
reject resolves quietly. The desktop renders the card with its
evidence and Start/Dismiss.

Deliberately narrow: task runs may also call `propose_task` now (it
does the sensible thing wherever called), but only reviews are
offered it and told about it. Review proposals are checked against
declined thread cards by the shared rule, but a declined inbox
proposal does not yet block a later review from proposing the same
title again; that ledger half is open work, not this change.
