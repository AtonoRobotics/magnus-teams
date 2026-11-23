# Routines check the holds of the agent that runs them

**Choice.** The routine sweep (`team/routine_sweep.rs::plan`) checks
`holds_new(author, false)`: the holds of the author's own agent, which is what
runs a routine (`start_room_mission_as` → `create_run` with no team run →
`refuse_new(author, false)`). A held routine is simply not due: it does not
fire, records no failure and sends no notice, and it fires on the first sweep
after the hold lifts. This is the rule a person's schedule already follows
(`halts::due_schedules`).

**Reason.** The sweep checked the team agent's holds instead. With the author
paused it fired anyway, the run was refused as `Halted`, the firing counted as
a failure and told the author, and after `pause_after_failures` firings the
routine stopped for good, so a temporary pause ended it permanently.
