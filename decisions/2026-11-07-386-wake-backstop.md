# The sweep backstops dropped event wakes (#386)

Codex P1 on #370: a task that ends while a review is active gets no
review at all — the fast wake in `after_end` is dropped when
`review_runs` already holds the person, with no retry.

The sweep now backstops it: alongside schedules, commitments and
approvals, the event computation considers the person's newest ended
task (not a review, not a chat turn) with no wake mark, ended over
thirty seconds ago so the fast wake had its chance. The newest first:
yesterday's unreviewed failure does not starve today's. The reason
reuses the fast wake's shape (`task {id} done|failed`), so the wake
mark lands from `ReviewWoken` exactly as for the fast wake, and the
`Ended` struct rides the same `start_review` parameter (#383),
label included, so a backstopped review starts tainted like a
fast-woken one (#390).

The thirty-second grace is a settling window, not a cadence: the fast
wake fires within milliseconds when no review is active, so anything
still unmarked after thirty seconds was dropped (active review) or its
review failed to start (create_run error, which also leaves no mark —
the backstop heals that too).

Test: `proactive.rs`
`a_task_ending_during_an_active_review_is_picked_up_by_the_backstop`
(fails without: the wake is dropped and no review ever names the
task). The boot review is stalled ten seconds so the failure is
guaranteed to land while it is active.
