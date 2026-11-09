# No double poll start: the six sightings were polls plus memory passes (#425)

The issue reported six in-window polls for three watches and proposed
mark-then-start. Reproduction plus the event log refuted the
mechanism: one sequential sweep loop drives polling, the advisory
lock holds one kernel per log, and the six sightings were three poll
runs (one per person, each marked immediately after creation, in
task-mark-task order) plus three memory-pass model calls whose
transcripts contain the Poll text. Two sweeps cannot start two
polls; there is no double start to fix, so mark-then-start (which in
any case could not fix concurrent sweeps — the mark lands only
after the log append the emit awaits) is not implemented.

What remains is a durable guard: a watch_poll test pins exactly one
poll run and one mark per person inside a one-minute window,
counting agent turns only. A mutation check (mark disabled) fails
it, so a future double start goes red here. The swallowed emit
result in start_poll (a lost mark on event-store failure would
re-poll every sweep) is real but unobserved — marks landed 3/3 in
every probe — so it is filed separately for its own fix rather
than bundled into this refutation.
