# A write to the log finishes even when its caller goes away

An append to the event log and its apply to the state now run on a task of their own (`commit::Writer`), which holds the write lock until both are done. The caller only waits for that task. If the caller is dropped, for example a client hanging up mid-request or a timeout around work that emits, the write still finishes and is applied.

Before this, the append and the apply ran inside the caller's own future. A drop between the two left the log holding an event the state had never applied. The next write saw the gap and restarted the server in place from the log (`log.rs`). Each such drop failed every open request for seconds. On magnus-teams, the desktop app's Apps page said the apps were unavailable and stopped answering.

Cancelling a write that has already gone to the database is not possible anyway: the append may already be committed. So the only safe choices were to finish it or to restart. Finishing costs nothing, and restarting cost every open request.

Events are still numbered and secrets typed into chat are still masked under the same lock, before anything is appended. The debug-only `crash-after` hook moved with the write, so the crash tests still end the process right after the append and before the apply.

The test `commit::tests::a_write_whose_caller_is_dropped_is_still_applied` drops a write while it waits on the database. It checks that the state keeps up with the log and that the next write follows on. When the write is run in the caller's future again, as before, the test fails.
