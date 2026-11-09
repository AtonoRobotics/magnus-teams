# Poll budget leaves retry headroom (#419)

A poll allowed 1 action: its read. A transient read error plus a
model retry hit the actions budget, suspending the poll with a
budget notice in the person's inbox — noise for an invisible run,
and a stuck poll until the person acts on a task they cannot see.
(Termination itself is ungated: task_complete bypasses the
pipeline, so the P1 as filed against #416 did not reproduce; this
is the adjacent real gap.) The budget is now 5 model calls and 3
actions: the read, one full retry, termination, and margin — still
half a review. A poll that cannot read after its retries completes
silently; the next interval polls again.
