# Schedules and routines share their arithmetic, nothing else

A person's schedule and a team's routine are different things: a schedule
acts with its person's authority and budget and records `schedule_*` events;
a routine acts with the workspace's standing authority, within the team's
spend limits, and records team changes. Merging them would blur whose
authority a run spends, so they stay apart.

What they share is when they are next due. Decided: that lives once, in
`cadence` (`Recurrence::{EveryMinutes, EveryHours, DailyAt}` and
`next_after(last, created, zone)`), pure, with the zone passed in.
`Schedule::next_due` and `Routine::due` use it. Counting from the last firing
means a server that was down for hours fires each one once on its return,
never a backlog. The daily clock-change rules moved with it unchanged, and so
did their test. No stored event changes shape.
