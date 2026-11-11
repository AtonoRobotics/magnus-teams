# The scheduled sub-agents scenario checks the schedule's cover, not Auto Review

`autonomy::scheduled_sub_agents::a_scheduled_run_uses_sub_agents` failed on main. Magnus had not regressed. The test was stale.

The test expected Auto Review to judge the implementation sub-agent's note change and be told it was "covered by a grant or schedule". Commit 8686f8f ("let a grant answer non-sends") changed `Kernel::settle` on purpose. Now a grant or schedule the person set up answers for them on an action that sends nothing out, and Auto Review is never called (decisions/2026-11-10-permissions-asks.md, case 1). 8686f8f updated the test's wording but kept the assertion that Auto Review saw the change, which can no longer hold. Before 8686f8f the test also failed, because its wording no longer matched the kernel's after 04ac0bd. Neither failure is in the password, push subscription, spend limit or poll commits.

The behaviour the owner asked for still holds: the sub-agent's change goes ahead under the schedule's standing authority, with no approval and no grant of its own. So the test now checks that:

- Auto Review is never called. The scripted Auto Review refuses everything, so a change that reached it could not go ahead;
- the developer view records the one allowed `notes__save` as "covered by the confirmed schedule sched_…".
