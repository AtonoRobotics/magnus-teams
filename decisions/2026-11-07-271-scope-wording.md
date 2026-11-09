# Scope wording names a schedule only for one (#271)

Since #253/#254, accepting an untimed suggestion or approving a task
proposal starts a one-off task carrying the card's scope. The context
told every run with a scope "The person confirmed this schedule for
this scope", so a one-off task was told it is a schedule.

The run is now told by what started it: `task.schedule` set keeps the
schedule wording byte for byte; without it, "The person approved this
task for this scope". The wording lives in one pure helper, unit-pinned
both ways; the appliance test pins the accepted-suggestion path
end to end. A schedule run's end-to-end wording is unchanged behavior
(the same string, now conditional) and is not re-pinned: firing a
schedule on this box's loaded runner is what #313 tracks.
