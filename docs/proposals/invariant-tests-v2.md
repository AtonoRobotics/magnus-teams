# Proposed changes to tests/invariants (needs the owner's approval)

A clean-context review (2026-10-27) found ways a non-compliant kernel could still pass the invariant tests. The kernel holes it found are fixed and covered by `tests/scratch/tests/review.rs`. These changes would move that coverage into the contract.

| # | Test | Weakness | Proposed change |
|---|---|---|---|
| 1 | inv1 `action_is_not_executed_when_its_intent_cannot_be_recorded` | The REVOKE also blocks the model-reply append, so the boundary is never reached; a kernel that calls before recording intent still passes. | Install a trigger that rejects only `kind = 'intent'` rows after the reply is logged; assert the `allow` decision was recorded and the app saw no call. |
| 2 | inv1 `kill_switch_stops_actions_and_release_resumes_nothing` | Satisfied by the loop's step-level check; boundary checks could be deleted. | Engage the switch while a call is inside the pipeline (hold the app's `tools/list`), then release the hold; assert no call. |
| 3 | inv6 `a_one_action_approval_covers_one_action` | The two sends differ, so the input hashes differ anyway. | Send the identical input twice. |
| 4 | inv6 `the_agent_proposes_and_only_the_owner_grants` | Only checks for no approval item. | Also assert `mail.effects("send") == 1` for the next task. |
| 5 | inv8 `only_holders_of_the_approver_role_can_approve` | Only matching risk tested. | Add a person with only `approver:irreversible` and a `financial` tool; assert 403. |
| 6 | new, inv4 | Memory, multi-label sources, broad approvals vs taint, tainted reads to wider apps. | Adopt the five taint tests from `tests/scratch/tests/review.rs`. |
| 7 | new, inv1 | Redirects to private addresses are never tested (tests allow private networks). | Add `Spec.block_private_web` and a test where a public-looking URL redirects to a private address. |
| 8 | new, inv2 | Credential echoes on error paths and in tool descriptions. | Fake app returns the bearer in a 401 body and in a tool description; scan model requests, log and output. |
| 9 | new, inv7 | Vacuous checks (memory, `equals: null`, missing pointer). | Adopt `checks_cannot_be_vacuous`. |
| 10 | new, inv1 | First-seen tool descriptions reach the model. | Fake app with an injected first description; assert it never appears in model requests before the owner approves it. |
