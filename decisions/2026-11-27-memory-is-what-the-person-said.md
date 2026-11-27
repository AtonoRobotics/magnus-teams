# Memory is what the person said, never the system's dialog

The owner, 2026-11-27: memory "was saving duplicates, every post, and
paragraphs of useless information", and status reports and run checkpoints
"should never be memories. They are system dialog."

Production on 2026-11-27 held 613 memories. 479 were filed from reviews,
schedules and kernel runs: "The person's latest review found a Wingstop
promotion; both recurring schedules remain paused...", a dozen a day. Others
were 2,000-character "RUN 1 CHECKPOINT" notes, and one preference was filed
9 times by hourly cadence reviews. The memory pass, a model call after every
run, cost 2,940 calls in 17 days.

Choice:

- The memory pass runs only on a run whose own text the person wrote (a
  chat turn, a task they asked for). A review, a heartbeat, a schedule's run,
  an opener and a sub-agent have none, so they get no pass and cost no call.
  Its prompt says Magnus's answer only explains the person's words: what
  Magnus did, found, reviewed or queued, and the state of schedules, checks,
  inboxes and tasks, are never filed.
- `memory_write` refuses, with the reason, a fact from a run in which the
  person said nothing, a fact over `MAX_MEMORY_FACT_CHARS` (300), and an
  empty one. The same fact in other case, spacing or punctuation is
  "already remembered" at any length; near duplicates (word overlap) as
  before from four words.

Reason: memory is for what the person taught Magnus about themselves and
their world. Magnus's reports to itself are already in the log and the
inbox. Filed as memory, they crowd the person's real facts out of
`memory_search`, which ranks newest first among equals.

Next (owner, same day): the old memory, events and their content rows, is
no longer used. Memory moves to the unified store, and everyone starts empty
there. The old entries stay in the event log as history and are not served.
