# The invariant tests no longer script the retired task contract

**Choice.** Spec rev 147 retired `task_contract` (`2026-11-12-evidence-not-contracts.md`); the kernel answers a call to it with `run::CONTRACT_RETIRED`. The invariant tests still opened about 116 scripted tasks with one, through the helpers `contract_checked` and `contract_unchecked`. They now open with `plan(goal)`, a one-unit `task_plan` call (optional, progress only), so each script's step indices and turn numbers stay the same. The criterion arguments are gone with the contract.

- inv5 `requested` accepted an app call as the model's decision when it was the check inside a contract the model wrote. With no contracts there is no such check for the kernel to run, so the clause is removed: every app call must be one the model asked for itself. That is stricter, not weaker. (Read-backs, the server's own declared reads after an irreversible action, do not occur in inv5's scenarios, which use reversible writes.)
- inv8_layers `a_sub_agent_never_holds_more_than_its_parent` named `task_contract` in the child's tool list; the kernel drops retired names, so it now names `task_plan`, which the child calls.
- inv7's assertions that no contract tool is offered and no contract is written stay. They are the only mentions of `task_contract` left in `tests/invariants`.

Also on this branch: the six-invariant and removed-parts patches were applied (see their records), the patch files that had landed were removed, and the `[legal]` section patch was removed unapplied (`2026-11-11-user-agreement.md`).

**Approval.** The owner, 2026-11-13: "Approve the invariants rewrite." and "owner doesn't patch you do."

**Proof.** The whole invariant suite ran on Strix Beta (`ci/on-strix-beta.sh inv-no-contracts`, one test at a time, `MAGNUS_CHROMIUM` set): 120 of 123 pass. The three failures:
- `desktop_screens` at line 88, known.
- inv2 `an_attachment_opens_only_in_a_per_run_sandbox` at line 529, the known intermittent guest connection; it passed when run again alone.
- inv3 `destroying_a_computer_loses_no_user_work` at line 326: the guest answered "Connection reset by peer". It scripts no model steps, so this change cannot reach it; it passed at the commit before this change and then 3 of 3 times on this branch. It is the same intermittent guest connection.
