# A batch of sub-agents is counted before it starts

**Choice.** Consecutive `start_sub_agent` and `research_web` calls in one
reply still run at once (2026-11-23-rich-agent-ux-bento-and-concurrency),
but the batch is admitted first, in one step (`sub_agents::batch`): as many
calls go ahead, first to last, as the run may still start sub-agents (the
cap of 8, and the depth) and as it has actions left (its tree's, and every
run's above it). The rest are refused up front with a tool error that says
why (`Denied (sub_agents)` or `Denied (budget)`). With no action left at
all, the first call goes alone and the boundary stops the run, as for any
action past the budget.

The actions given to a batch are held (`spend::holds`, `Holds::reserve`)
until the batch is done: they count against every other run of the tree,
so a sub-agent the batch started cannot take them first, but never against
the batch's own calls. Each action the run records uses one of them, so a
batch that waits for its children does not keep them from what is left.

**Reason.** Each call checked the cap and the action budget, then waited on
other work before its child or its action was recorded, so a reply with 20
calls started 20 children at every depth and passed the action cap.
