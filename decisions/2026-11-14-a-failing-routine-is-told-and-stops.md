# A failing routine is told, does not starve the others, and stops

Found in the architecture review (schedules and routines). `run_due_routines`
returned on the first routine that could not start (any error but a team
spend limit), so every routine after it in that pass started nothing, the
failure was recorded nowhere, and it was tried again every minute, silently.
Nor did it check whether the routine's last run was still open, so a run
longer than `every_hours` overlapped the next.

Decided, the same rules as a person's schedule (kernel sweep), now decided by
a pure `team::routine_sweep::plan` the kernel only carries out:

- A routine that cannot start is recorded as `routine_failed` (a new team
  change) with a notice to its admin, and the pass goes on to the next
  routine. A failure is never silent (K24). Like `schedule_failed`, it counts
  as the routine's firing: it is tried again one interval on, not each minute.
- Not while its last run is open: the firing is recorded as `routine_skipped`
  and starts nothing, as `schedule_skipped` does.
- After `schedules.pause_after_failures` firings in a row fail (could not
  start, or the run ended failed; the count that pauses a schedule), the
  routine stops: `routine_stopped`, with a notice to its admin. It starts no
  more runs until its admin changes it (`routine` again, which also restarts
  the count) or removes it. Reason: the same rule as schedules, and the safer
  reading, a failing routine must not keep spending. Routines have no pause
  or resume, and none is added.

A spend limit that holds a routine back still only waits, as before.

Existing kinds are unchanged; the firing history and the stop are kept in
state only (`#[serde(skip)]`), rebuilt from the log. Tests:
`team::routine_tests::a_routine_that_cannot_start_is_told_and_the_rest_still_start`,
`team::routine_sweep::tests::a_routine_does_not_fire_while_its_last_run_is_open`
and `..::a_routine_whose_firings_keep_failing_stops_until_changed`.
