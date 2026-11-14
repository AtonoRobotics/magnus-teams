# The schedule sweep is planned from state, then carried out

The parked half of candidate 9 (decisions/2026-11-13-an-ended-schedule-is-never-due.md),
unblocked now that the erasure work on `State` has landed.

Decided: what the sweep does with people's schedules is decided by pure
functions over `State` in `schedule_sweep` (`ends`, `holds`, `firings`,
giving `End`, `Hold`, `Release`, `Skip`, `PauseFailures` and `Fire` steps),
and `Kernel::sweep_schedules` only carries the steps out. The three phases
are planned one after another, each from the state the steps before left,
as the sweep always ran them, so behaviour is unchanged: an ending schedule
is not held or fired, a held one is not fired. What must be checked when a
step is carried out stays there: a release re-reads the hold (#318), and a
firing re-checks paused or ended after its words are recalled, with no await
between that check and `create_run`. The debug sweep and release gates sit
where they did. The kernel's sweep is one call; `kernel.rs` falls to 800
lines and leaves `ci/oversize.txt`. Tests: `schedule_sweep_tests`.

`api.rs`'s `schedule_rule`/`rule_for` still read schedule timings themselves;
they should later go through `cadence` (left alone: route handlers are being
rewritten in parallel).
