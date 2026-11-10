# Sub-agents carry their parent's authority

The owner, 2026-11-10: "A sub-agent carries its parent's authority: grant,
confirmed scope (including a schedule's), project, requester, and the approvals
given to the parent's task. It needs no grant of its own; the parent only
narrows it by tools. Remove the grant input from start_sub_agent." Spec rev 140
agrees: "A new source, a tool, a task, a schedule, a retry or a sub-agent never
asks on its own." This replaces the grant rule in 2026-11-09-sub-agents.md.

## The choice

Authority is looked up through the parent chain wherever it is checked, so it
is never copied onto the child where a copy could drift or widen. The chain is
`Ev::SubAgent`, now written before the child's `TaskCreated`, so the child's
first call already has its parent. The helpers are in `sub_agents.rs`:
`State::authority` (the top run's task), `State::with_authority` (the child's own
task with the top's grant, schedule and project), `State::lineage` (the child's
task and every parent's) and `State::requester_run` (the top run).

| Inherited | Where it is enforced |
|---|---|
| Grant | `covering_grant` (boundary.rs), `grant_spent` (grants.rs), the charge in `State::charge` |
| Confirmed scope, a schedule's included | `what_covers` (auto_review.rs) takes the top task's schedule; the child is told the top task's scope (`start_sub_agent`), as a schedule's (`context_with` with `with_authority`); a scheduled run's child cannot set up another schedule |
| Project | `context_with` and `offered` (run/mod.rs), `project_file` (boundary.rs) |
| Requester | the person is the parent's (`create_run`); the person's own words are the top run's in `named_by_person` (boundary.rs), `said_in` (commitments.rs) and what Auto Review is told the person asked. A child's goal is the parent model's words, never the person's. A child whose top run is gone has no one's words. The team scope is copied as before. |
| Approvals | `covering_approval` (boundary.rs) matches `once` and `task` approvals, and the web-lookups approval, against `lineage` |
| Background, unattended | `is_background` (kernel.rs) and the unattended ask expiry (unattended.rs) take the top task |

The parent narrows a child only by naming tools: each must be one the parent
holds (`every_tool_for`) and within its own limit, or the start is refused
(`narrowed`). The boundary checks the limit of every run above the child
(`Ledger::refusal`), and the tools offered to a child leave out what that limit
refuses (its run records stay).

A child's spend is the parent's. Its model calls are charged to the parent's
grant, and each run above it adds the cost to `sub_usd`. Budget checks use
`Run::spent_usd` (own plus sub-agents'). A child's budget is a share of what the
parent has left, not of its limit. A child's failure returns to its parent as
the tool's result, so it puts no notice of its own in the person's inbox: the
parent's failure would.

## The retired input

`grant` is gone from the schema and the handler. A call written before this
change (a pending tool call replayed after an update) may still carry it: the
handler's input type ignores it, so it is read as nothing and the child works
under the parent's grant. Children started under the old code carried a grant
equal to their parent's or none, so looking the grant up through the chain gives
the same answer for them.

## Why

Looking authority up through the chain means a child can never hold more than
its top run holds now. A grant revoked or spent, or a schedule cancelled,
binds the children at once. Copying would let them keep it. Reading the
person's words from the top run closes a gap in the old code: there a child's
goal, which the parent model wrote, counted as the person's own words for
recipient provenance and commitment checks.

## Proof

Unit tests in `sub_agents.rs` cover every inherited field, spend against the
parent's grant and budget, narrowing, refusal of widening, and the retired input.
Each failed with the old code put back. Acceptance scenario 6
(`tests/scratch/tests/autonomy/scheduled_sub_agents.rs`, in the autonomy suite)
runs a schedule that starts three sub-agents with no grant input.
