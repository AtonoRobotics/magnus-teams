# Scheduled runs post in the conversation where the schedule was set up

The owner, 2026-11-10: "Scheduled runs post in the conversation where the
schedule was set up, and a failed run wakes the agent there." Spec rev 140 says
the same: an outcome "returns as a new turn to the agent in the conversation
where the work or schedule started". This updates
2026-11-09-outcomes-return-to-the-agent.md, where a scheduled run posted in the
schedule's own thread, `schedule-{id}`. The behavior spec's earlier "each
schedule run posts to the schedule's own thread" is superseded.

## The choice

- `ScheduleSet` gains an optional `thread`: the conversation of the chat card
  the person confirmed. Old events have none (serde default), so no event kind
  changes.
- Each run's task starts in `schedule_thread::of`: the recorded conversation,
  while it is neither trashed nor removed; otherwise `schedule-{id}`. The result
  posts there, and its outcome turn wakes the agent there, through the
  outcome-turn machinery of c357f32 and its retry limit, unchanged.
- A schedule with no conversation keeps its own visible thread: one set up in
  settings, one accepted from the inbox, one set up before this change, and one
  whose conversation is gone.
- A failed scheduled run still puts a notice in the person's inbox, as before.
  Nobody is there when it fails, and a failure is never silent (K24). A failed
  task the person started in chat has no notice, because the person is in that
  conversation. The two cases differ, so the notice stays.
- A retry the agent starts from that outcome turn runs as any retry does
  (c357f32), with the ended run's grant, scope and budget.

## Proof

`schedule_thread.rs` unit tests cover the recorded conversation, the old
schedule's own thread and the removed conversation. The first failed with the
old code put back. In acceptance, scenario 2 now finds the result and report in
the conversation and no `schedule-{id}` thread. Scenario 6's second test has a
failing scheduled run wake the agent there with "retry 1 of 2".
`tests/scratch/tests/schedule_threads.rs` covers both kinds of schedule.
