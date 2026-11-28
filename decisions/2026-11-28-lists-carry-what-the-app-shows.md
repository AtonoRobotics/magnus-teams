# The task and inbox lists carry only what the app shows

The owner, 2026-11-27: "There is now a very long delay before agent
responds to a prompt."

The server answered in two to four seconds; the app showed the reply
twenty-five seconds later. A capture on port 7800 showed why: in fifty
seconds the app downloaded 45 MB over Tailscale. Each refresh fetched
`/v1/tasks` at 2.4 MB, every one of the person's 2,000 proactive review
runs, which the app then threw away (`tasks.filter(t => !t.review)`), and
`/v1/inbox` at 0.75 MB, mostly observations the person had dismissed. A
run changes the person's ETag with each step, so each step fetched both
again, and the refresh after a send waited behind them.

Choice:

- `/v1/tasks` and the task search leave out background runs, reviews and
  watch polls (`Task::background`), as polls already were (#406). They
  stay in the log and the developers' run inspector.
- `/v1/inbox` leaves out resolved observations and interruptions. A
  dismissed one is never shown again (decision of 2026-11-28 on Clear
  all). Resolved asks stay, for the approvals history.

Reason: what a list carries grows with what the server does in the
background, and the app reads it on every change. A list holds what the
person sees.
