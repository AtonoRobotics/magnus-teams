# Proactive loop core (#237)

Built the loop core: the sweep wakes one review run per person on cadence
in working hours, once overnight, and on events (task done/failed, stale
approval, coming-up schedule, due commitment). The review decides nothing,
`observe`, `interrupt`, propose, or act; observations and interruptions are
inbox items with `notify_at` gating Needs-you attention. Working/quiet
hours run in the person's timezone preference; the daily interruption
budget counts delivered interrupts; `[proactive].on` is the kill switch.
Split out: #367 (app-home what-changed + opened tracking), #368 (Cedar
suggest/interrupt/act + suggestion preferences), #369 (new-app-item wake).

Took over from a stalled peer claim (unpushed part-1 commit, idle 9h, no
PR): the issue's "wakes a review run" settled the architecture as runs,
not chat turns. Their "brief" idea is salvage for #367.

Tests: `proactive.rs` e2e (observe lands, out-of-hours queues till
morning, quiet posts now/pings later, budget downgrades the second
interrupt, kill switch runs nothing) 5/5; lib 110/110 incl. window/budget
units; settings 6/6; cancel_report 2/2. chat_controls 9/11 with the same 2
taint failures as pristine (verified via stash).
