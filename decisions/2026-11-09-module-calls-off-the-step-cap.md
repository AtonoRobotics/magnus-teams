# The kernel's own model calls do not use up the agent's steps (2026-11-09)

The only scheduled run in the teams server's log (run_47df5019…, schedule
sched_b915319c…, "Test scheduled runs") stopped for budget with "Stopped
after 3 model calls, the task's limit." The agent had made two calls: the
contract, then reading the time. The third counted call was the verifier's
review of its criteria. The agent never got the step that would post the
confirmation, the run waited on its person, and the schedule paused.

- The loop's call cap (`max_model_calls`) now counts only the loop's own
  calls (`Run::loop_calls`, `run/loop_calls.rs`). Calls of the verifier,
  reviewer, memory pass, knowledge extraction, read aloud and scoped
  drafting are counted apart (`Run::aux_calls`). Reason: #319 made them
  optional extras that "never suspend the run", yet each took one of the
  agent's steps and so could suspend it. A criteria review that costs the
  task its delivery defeats both.
- They still count toward the run's dollars and the grant's, and still skip
  themselves once the run's total calls reach the cap, so a run's total is
  at most the cap plus the modules' calls made before it. Reason: the
  dollar budget is what bounds money; nothing promised about spending moves.
- `model_calls` stays the total for the activity and developer views.

Not taken here: the schedule's budget of three calls was the model's
estimate when it proposed the schedule; budgets set by the server are a
separate change.

Test: `run::loop_calls::tests::a_verifier_call_does_not_use_up_the_agents_steps`.
