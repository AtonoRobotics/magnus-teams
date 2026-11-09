# Poll retry headroom (#419)

One-line budget change (3/1 to 5/3) plus doc. Test in
`watch_poll.rs` with a fail-once mock (armed after the seed): the
poll's first read errors, the retry reads the new message and wakes
"1 new", and no budget notice reaches the inbox. Failed before on
the inbox assertion (retry suspended, notice posted); passes after
in ~61s solo. Full watch_poll 3/3; gate: proactive 9, reviewer 6,
new_items 4, lib 116, inv4 11, inv7 5, workspace check clean.

Test-writing notes: the fail counter must `swap(0)`, never
`fetch_sub` (it underflows past zero and errors forever — this bit
once here); with three people per appliance the wake may come from
any person's poll, so the inbox assertion (no suspension anywhere)
is the discriminating one, and it holds because exactly one poll
sees the armed error and every person is seeded.
