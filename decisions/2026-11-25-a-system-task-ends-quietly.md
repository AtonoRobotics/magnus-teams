# A system task that finished well wakes no review

## Decision
When a system task (the heartbeat, or any schedule with `system: true` or
the `system_task` module) completes, its end no longer wakes the person's
review. A failed one still wakes a review, as any failed task does.

## Reason
A system task is itself a review: it reads the person's apps and work and
stays silent unless they are needed (2026-11-24, heartbeat as a quiet
system task). The review its end woke read the same state again. In
production on 2026-11-25 each of the heartbeat's 27 runs woke one, 1.3M
input tokens for observations the run itself had already weighed. The
owner, the same day, asked for both this and the heartbeat's removal after
seeing that the heartbeat duplicated the built-in review loop.
