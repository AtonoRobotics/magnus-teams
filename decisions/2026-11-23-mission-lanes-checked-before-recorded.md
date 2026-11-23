# 2026-11-23: A mission's lanes are checked and admitted before anything is recorded

## Choice
`Kernel::check_lane` runs every check a lane must pass, its objective (length, `scrub_check`), its scope, whose agent it runs for and their place in the room, and whether that person's run would be admitted now (the halts and `admit_run`, the same checks `create_run` makes). `start_mission_with_lanes` checks every lane, and `start_room_mission_as` its one lane, before `Started` is written. If any lane fails, nothing is recorded and nothing starts.

## Reason
`Started` was written first and the lanes were checked one at a time as they were dispatched, so a request with lanes [valid, bad] answered 400 with the first lane already running and spending, and a person at their limit left an empty mission that could never complete. A run can still be refused between the check and its start if a limit is reached in that moment; that narrow race leaves at most a mission whose lane failed to start, the same as before for that one case.
