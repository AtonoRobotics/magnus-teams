# Background work asks, sets up schedules, and reaches everyone

A review against spec rev 142 found Magnus's own work held back where the spec lets it go. The spec wins over the owner's 2026-11-07 rule that background work asks no one. These follow from it.

## Background work asks when Auto Review says ask

"On each event Magnus decides for itself whether to suggest or to act ... It asks only when Auto Review says ask." And: "An ask from work nobody is watching waits 30 minutes, then a message stays a draft and other work waits on the ask, while everything that does not depend on it continues."

A review or a watch poll (and their sub-agents) now raises its ask like any run. The card goes to the person's approval list, and the run waits on it (`background_work`). After 30 minutes `unattended` handles it as it already did for schedules: a message stays a draft and is not sent, anything else keeps its card open, and the run carries on with what does not depend on it. Any other stop of background work (a question, a sign-in, its budget) still ends it quietly, as before: only the four asks reach the person.

A review waiting on an ask holds no later review for its person. Otherwise one unanswered ask would stop every new email from being looked at for 30 minutes, and the spec says what does not depend on the ask continues.

The team agent's own routines still ask no one. The spec gives them the workspace's standing authority and has them report to the channel. They have no person of their own to ask, and the team area is another agent's.

## Tasks and reviews set up schedules

"A new source, a tool, a task, a schedule, a retry or a sub-agent never asks on its own." A task or a review is offered `create_schedule` and `list_schedules`, as chat is. Outside chat a schedule must end (`max_runs` or `until`): nobody asked for it in so many words, and a schedule with no end, set up by work nobody watches, is the runaway the 2026-10-30 rule guards against. Its runs post in the conversation the work came from, when there is one.

A scheduled run (or its sub-agent) still sets up no schedule. Schedules would breed (2026-10-30), and the person who set up the schedule asked for its goal, not for more schedules. `a_scheduled_run_cannot_set_up_a_schedule_and_its_schedule_waits_for_it` keeps testing the suggest path; the new scratch test covers `create_schedule`.

## Everyone gets polls and wakes

The proactive loop iterated the config's people only, so people invited since never had their apps polled or their reviews woken, while their harness told them their email woke Magnus. It now iterates `everyone()`, as Admin › Agents does. The harness names watched apps only when polling them actually happens: proactive on and the person not turned off.

## One task end makes one run

A task whose outcome returns to the agent as a turn in its conversation (`outcomes`), and a sub-agent whose outcome returns to its parent, no longer also wake a review. The agent already has the outcome there; a second run read the same result and spent the budget twice.

## A model with no price is not free

The OpenAI-compatible adapter counted a model with no entry in `prices` as $0. 560 `gpt-6.1-sol` replies were recorded at $0 on the teams server, so the per-task limit never saw them. "Its budget is the server's per-task limit, never a model's estimate." The adapter now refuses to quote an unpriced model, and the kernel fails closed: a model call it cannot price is not made, and the run stops for its budget with a notice that says so. A free model is priced 0 in config, explicitly.

## Wording

The task instructions no longer tell the agent that the person sees each check labelled: "That checking is internal: the person sees the result, not criteria or contracts." The harness says server, not appliance.
