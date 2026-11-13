# Evidence comes from the record, not from task contracts

Approved by the owner on 2026-11-12 (spec rev 147, "Evidence comes from the
record, not from ceremony", and "How a task looks").

Choice:
- Remove task_contract and the rule that it comes before any action, contract
  freezing, contract-change approvals, criteria checks written as JSON-pointer
  assertions, the verifier's power to reject a completion, and propose_grant.
- A result carries receipts from the boundary log, a server read-back after
  each irreversible action, and sources linked to its claims. The agent checks
  its own work the way the work allows and says in one line what it could not
  check. task_plan stays as optional progress, never a gate.
- A task is one card in its conversation: title, live status line and Stop,
  becoming the result when it ends. The full timeline is in the developer view.
- Invariant 7: the owner approves replacing
  no_completion_and_no_action_without_a_contract. The false-completion and
  read-only checks stay, retargeted to read-backs. The patch still goes to the
  owner to apply, because tests/invariants/ is protected.

Reason: Muse, Claude Cowork and Codex show a plan as progress, an activity log
and cited outputs, with no pre-declared contract. For most non-code work the
criteria are judgments, so the checks became paperwork the owner had to
review. The real danger, a false claim that something was done, is caught by
reading the world back.
