# An ended schedule is never due again

Found in the architecture review (candidate 9, schedules): `Schedule::next_due`
checked `paused` and the run limit but never `ended`. A schedule one of its own
runs ended through `end_schedule`, with no end time and no run limit, stayed
due. While that run was open the sweep skipped it, and once the run finished
the sweep fired it again, while its card said it had ended. The re-check after
its words are recalled tested only `paused`, so it did not catch this either.

Decided: an ended schedule is never due (`next_due` returns nothing once
`ended` is set), and the sweep's re-check refuses an ended schedule as well as
a paused one. Ending is already terminal in state ("no pause outlives it");
this makes it terminal for firing too. The test is
`schedule_thread::tests::a_schedule_its_run_ended_is_never_due_again`; it
fails without the fix.

Moving schedules out of `State` and the kernel sweep (the rest of candidate 9)
waits until the erasure work on `State` lands.
