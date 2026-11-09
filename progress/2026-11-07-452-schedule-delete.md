# Ended schedules get Delete (2026-11-07)

- `desktop/ui/app.js`: `scheduleButtons` gives an ended schedule Delete schedule (`delete-schedule`, with its own confirmation) in Schedules and the schedule itself; `scheduleLine` gives its chat card Delete, confirmed as a delete.
- Test: `a_schedule_that_keeps_failing_pauses_until_its_person_resumes_it` adds a one-run schedule, waits for it to end, and `desktop/tests/schedules.mjs` (new `MAGNUS_ENDED`) checks it shows Delete schedule and no Stop schedule, and that Delete removes it. Fails without the change (no such button), passes with it.
- Review fix (Codex on #454): `controls.rs` `cancel_schedule` leaves an ended schedule's runs going. Test: new `deleting_an_ended_schedule_leaves_its_last_run_going` in `tests/scratch/tests/schedules.rs`, which fails without it (`runs_stopped: 1`).
- `desktop/tests/live-cards.mjs` checked that an ended schedule's chat card offers no acts, the rule this fixes; it now checks that Delete is its only act.
