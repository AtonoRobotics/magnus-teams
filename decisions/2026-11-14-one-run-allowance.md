# One allowance answers whether a run may spend more

A review found the question "may this run spend more?" answered by hand in
seven places, each checking a different set of rules. The agent loop checked
the call cap, the run's dollars, the grant's budget, the month's limits and
the team's. The boundary checked actions and dollars only. The reviewer and
the memory pass checked calls and the grant. Knowledge extraction checked the
grant only, and spoken scripts the calls only. So the kernel's own model
calls and the model's actions never saw the month's or the team's limits
partway through a run, which broke the 2026-11-10 and 2026-11-13 rule that a
limit holds partway through work.

**One module holds every rule** (`spend/allowance.rs`). `allowance(state,
run, spender, now)` answers with what is left, or a stop with its reason,
notice and details. Each spender asks it once: the agent loop before each
model call, the boundary's budget stage before each action the model asked
for, and `aux_max_tokens` before every extra call (reviewer, Auto Review, the
outgoing classifier, memory pass, knowledge extraction, spoken script).

**The spender says which call cap applies.** `Step` is the agent's own call,
counted by `loop_calls`. `Extra` is one of the kernel's calls, counted by all
the run's calls, so extras never use up the agent's steps but stop once the
run's total reaches the cap (2026-11-09). `Action` is held to the action cap
instead. The dollars, the grant's budget, the month's limits and the team's
limits bind all three alike.

**What changes for each caller.** Extras now skip at the month's and the
team's limits, and knowledge extraction now skips at the call cap like the
other extras. The boundary now stops an action at the grant's, the month's
and the team's limits, with the same reason and notice the loop gives
(`grant_budget` or `spend_limit`). The stops' words and details are as they
were. Auto Review and the outgoing classifier fail closed when they make no
call, so a spent limit sends the action to the person, never past them.

Not changed: what is left in dollars is still the run's own budget. A run's
budget is already cut to the month's and the team's remainder when it starts.

Reason: one rule set, asked once, cannot drift between callers. The limits
the spec promises hold for every call that spends money.

Tests: `spend::allowance::tests` (each rule for each spender from plain
events; the boundary stopping at the person's month limit and at the
workspace's limit).
