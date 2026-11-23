# A sub-agent tree shares its parent's limits

The third architecture review (candidate 11) found that only dollars were
counted across a sub-agent tree. Each child was given a share of its
parent's whole action and model-call limits, each run was checked against
its own counts only, and nothing limited how many children a run started.
A task with a 40-action limit could start any number of children with 20
actions each, held back only by dollars.

**The tree's totals are counted in one place.** `spend::tree` adds up the
actions and model calls of a run and every sub-agent below it. The
allowance checks the run's own budget and the budget of every run above
it, each against its whole tree: the run's model calls, actions and
dollars, and the dollars left of each run above (less what calls in flight
below it hold). A child within its own share is still stopped once the tree
reaches the top's limit, and so is the top itself.

**A child's budget is a share of what is left.** Its model calls, actions
and dollars are each a share of what the parent's tree has left, as the
2026-11-10 decision already said for dollars.

**A run starts at most 8 sub-agents** (`MAX_CHILDREN_PER_RUN`). The spec
gives a child "a share of the parent's budget" and says nothing on breadth;
depth was already capped at 3. With the tree's totals counted, the cap is
a second bound, so a run cannot fan out into hundreds of runs that each
cost a first model call before the totals stop them. `research_web`, which
starts a sub-agent too, is held to the same depth and breadth.
