# One ledger for money

**Choice.** What a model call cost is written on a ledger in the state
(`spend::book`) as the usage events fold, by month, person, agent, model,
project, workspace and channel. The month's totals, the team's workspace
and channel totals and every limit read the ledger, never the runs.

- Trashing a task, removing it for good and erasing a person do not touch
  the ledger. An erasure moves the person's lines under `(erased)` and
  drops their project: their dollars stay in the server's and the team's
  totals, and the ledger names no one erased (`forget_tests.rs`). The id
  is never given again (2026-11-11), but the name still goes, as every
  ledger forgets its person (2026-11-13).
- A cost counts in the month it was spent, not the month its run started.
  A team run's cost counts against its workspace and channel from when it
  is one (all team runs are linked at creation).
- The log already keeps every usage event (content is scrubbed, events are
  not deleted), so a replay folds the same totals.
- A call's allowance (`Left.usd`) is the least of what the run's budget,
  its grant, the person's month, the server's month, its workspace and its
  channel have left. The agent loop fits each call's worst case to it, so
  the "extend" answer, which doubles the run's own budget, cannot carry a
  run past any other limit; the extension is left as it is.
- Calls in flight hold their worst case (`spend::holds`, in memory only)
  from admission to the fold of their cost, and admission reads what is
  left and takes the hold under one lock, so two calls at once cannot pass
  a limit together. A call that does not fit only for what others hold
  waits for them, up to five minutes, before its run stops; one that
  would not fit anyway stops at once.

**Reason.** Month and team spend were the sum over runs that still
existed, so a person at their limit could trash conversations and spend
again, and an erasure gave the server's month back. Erasure removes
words, not money. The per-call clamp read only the run's own budget, so
a call could pass the month or a channel's limit before the next check
refused, and two runs could each spend the same dollars.
