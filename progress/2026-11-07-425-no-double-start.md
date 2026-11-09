# Double-start refutation and guard test (#425)

No product change: the reported double poll start does not exist.
`tests/scratch/tests/watch_poll.rs` gains
`a_due_watch_polls_exactly_once`: one connected watch, three
people, twenty seconds inside a one-minute interval, asserting
three agent poll prompts and three `watch_polled` rows. Verified
green (20.5s) and red-capable (mark disabled: fails on the poll
count). The swallowed mark emit is filed separately.

Probes used and reverted: per-prompt turn/system logging and
in-test event-log queries, which showed three poll runs with
immediate marks plus three memory passes carrying the Poll text.
