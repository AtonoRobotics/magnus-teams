# The sweep rechecks a release before emitting it (2026-11-06)

- The release loop re-reads hold-ended state at emit time (no longer held, still paused on the run) instead of emitting unconditionally from its snapshot (#318). Reason: the hold notifications await between the snapshot and the emits, and since #262 a pause can land in that window; the stale `ScheduleResumed` undid the fresh personal pause. Same recheck-at-emit pattern as #261's firing guard.
- The new gate shares one debug-only helper with the firing gate (`debug_gate`), keeping the `<state_dir>/<gate>` + ack file protocol. Reason: two copies of the wait loop would drift; the firing gate's files and behavior are unchanged.
