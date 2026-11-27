# The heartbeat is retired

## Decision
The heartbeat is gone from the product:
- The app no longer offers it: the "Enable Heartbeat" button in the side
  panel's Up Next, and the Heartbeat card with its "Enable (30m)" button in
  Schedules, are removed.
- A heartbeat still set up (a schedule with `system`, or one whose words
  name a heartbeat, as `outcomes::is_heartbeat_text` knows them) ends the
  next time it comes due instead of starting a run: the sweep writes
  `ScheduleEnded` with the reason `HEARTBEAT_RETIRED` and tells its person
  once, as any ended schedule does. Run now on one ends it the same way and
  starts nothing.
- `system` on a schedule's record and the `system_task` module stay, so
  events already written read as before.

## Reason
The owner, 2026-11-25, asked for the heartbeat's removal after seeing it
duplicate the built-in review loop (hourly cadence, overnight, arrivals,
stale approvals, commitments) at about 41% of production's input tokens.
Only the other half of that request landed (2026-11-25, a system task ends
quietly). Deleting the owner's own schedule needed their sign-in, and no
CLI ends a schedule, so it kept running every 30 minutes, and the app kept
offering to start a new one. On 2026-11-26 the owner said again that it was
meant to be removed. Ending it in the kernel reaches every heartbeat,
including ones made before schedules carried `system`, without anyone
signing in.
