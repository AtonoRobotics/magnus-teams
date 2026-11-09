# Sub-agents

**A child is a run.** `start_sub_agent` makes a new run with its own context and goal, and `Ev::SubAgent` records its parent and the tools it may use. It keeps the requester as its person, so its approvals and its entitlements stay theirs.

**It holds no more than its parent.** A grant the parent does not hold is refused. A tool list only narrows, and a nested child cannot widen its parent's list. The list is checked at the boundary, not just where tools are offered. Depth stops at 3, and a child gets a share of the parent's budget, at most half.

**Its result is data.** The parent's tool result carries the child's taint, so what the child read taints the parent.

**A team run does not start one yet.** A child would need the team scope carried with it, and until it is, a child in a team thread could reach personal context. It is refused.
