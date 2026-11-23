# Ending a run ends its sub-agents

**Choice.** One operation ends a run and its subtree. `Kernel::cancel_tree`
(`run_end.rs`) writes the run's own `Cancelled` (and what is said with it) and
a `Cancelled` for every sub-agent below it still going, in one write, then
each run it ended lets go of what it held. Every cancel path uses it:
`cancel_task`, `cancel_schedule` (whose filter on `schedule` never found the
children, which carry none), the superseded chat turn and the team's
`end_team_tasks`. A run that ends any other way (done, failed) ends its
sub-agents in `end_run`, which `on_stop` calls for every terminal stop.

The children's `Cancelled` events are written by `kernel` with no source: the
person stopped the top run, not each child, so a child is not counted as
stopped by a person (`stopped_by_person`, the regret measure).

A backstop holds where the agent loop decides on its next step
(`stopped_from_above`, called at the start of `step` and in `may_ask_model`
before each model call attempt): a run with any run above it that has ended
asks the model nothing and takes no action, and ends `Cancelled`. Any run
above, not only the top, since a child below an ended middle run has no one
waiting for it. A parent that is no run (a mission lane's requester) never
ends. It is not in `spend/allowance.rs`: the step is where the loop decides,
and the allowance is about money.

The events are explicit, so replay needs no fold that cascades.

A child waiting on a person while its parent ends is now cancelled. The parent
is told so when it hands back: "It carries on once they answer, unless this
run has ended by then."

**Reason.** Only the top run was cancelled: its sub-agents went on acting
under an authority that had ended, against
2026-11-10-sub-agents-carry-parent-authority.md ("a schedule cancelled binds
the children at once").
