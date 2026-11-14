# A sub-agent is held to its top run's loop policy

**Choice.** The loop policy (`suggest`, `interrupt`, `act`, #368) governs a sub-agent of a review or of a scheduled run exactly as it governs that top run. Asked "A sub-agent of a review or a scheduled run is not governed by the loop policy (interrupt, suggest) that governs its top run. Should it be?", the owner answered "yes." (2026-11-13).

Whether a run is governed is now a question of its kind, `RunKind::loop_governed`: a review or a scheduled run, or a sub-agent whose top run is one, looked up through the parent chain. The whole verdict moved to `State::loop_deny` (`run/kind.rs`), which `Kernel::loop_deny` calls with the run's person; every caller (interrupt, observe, suggest, the boundary's `act` gate) goes through it unchanged. A sub-agent is judged with its own taint (`untrusted`, `private`), which is at least its parent's. It stays fail-closed: a run whose task the log does not hold, a sub-agent whose top run's task it does not hold, or a governed run with no person, is denied. Person-directed work (a chat turn, a task, a team request, a watch poll, or a sub-agent of one) is still not the loop and is allowed.

`run/kind_tests.rs` gains a `loop governed` column in the table of every kind, and `a_sub_agent_is_held_to_its_top_runs_loop_policy`, which fails on the old reading: with the org forbidding `interrupt`, `r_sub_review interrupted where the org forbids it`.

**Reason.** The 2026-11-10 decision says a sub-agent carries its parent's authority and that authority is looked up through the parent chain wherever it is checked. `loop_deny` read the run's own task, whose `review` and `schedule` a sub-agent never has, so a review could do through a sub-agent what its policy forbids it to do itself.
