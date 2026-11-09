# #318: stale sweep release cannot undo a fresh pause

The release loop rechecks (hold ended, still paused on the run) before emitting `ScheduleResumed` (`kernel.rs`); the firing gate was factored into a shared debug-only helper with a new release gate for the test.

RED: `a_pause_after_the_release_snapshot_is_not_released` fails without the recheck ("the stale release undid the pause"). GREEN with it.
