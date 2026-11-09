# #261: sweep starts no run for a schedule cancelled or paused mid-flight

The sweep rechecks schedule liveness right before `create_run` (`kernel.rs`), with a debug-only `sweep_gate` the new tests use to land the cancel/pause deterministically mid-flight. Two tests in `tests/scratch/tests/schedules.rs`: cancel-while-starting gets no run; pause-while-starting gets no run until resumed, then fires normally.

RED: `a_schedule_cancelled_while_starting_gets_no_run` fails on unfixed code ("a run started for the cancelled schedule"). GREEN: both new tests pass with the fix; `magnus` lib tests 84/84. Full `schedules.rs` file and CI pending at commit time.
