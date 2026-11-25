# A routine's lane asks no one

**Choice.** A lane that a routine fires is marked as that routine's when it
is dispatched: `LaneSpec::routine` (server-set only, never read from a
lane's body), recorded on `LaneDispatched` as an optional `routine` field and
on the lane. `State::background_asks` is false for such a lane's run, as it
was for a team agent's own routine. So something that would ask is refused,
and the agent reports to the channel and finishes. The field is set by the
routine sweep alone. Quiet-thread follow-ups and ordinary asks pass none and
still ask their person.

The lane is recorded before its run wakes, so the mark does not depend on
`RoutineRan`, which is emitted only after the mission starts. The new field
defaults to none, so older `lane_dispatched` events read as they did.

**Why.** `2026-11-22-retire-team-agent-runs` says a routine fires as "background
work that asks no one, as a routine always was". After it moved onto the
author's lane, `background_asks` saw no team run and let the lane ask. Its
author was not watching, so the routine waited and never reported
(invariant 8, `the_team_agents_own_work_reports_to_the_channel_and_asks_no_one`).
