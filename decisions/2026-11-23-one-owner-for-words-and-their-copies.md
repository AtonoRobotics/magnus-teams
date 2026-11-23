# 2026-11-23: One owner for a person's words and every copy of them

**Choice.** `team/copies.rs` is the one place that knows where a person's
words are copied. Erasing a person and deleting a message for everyone both
ask it which events hold copies, name those events in their `Removed`, and
delete their content with the rest. `State::drop_copies` runs when a
`Removed` is applied, live and on every replay, and takes out of the state
what those events put there.

- **A whisper reads its thread, it does not copy it.** A whisper used to
  copy the last 40 messages of its thread, everyone's, into its own event,
  which is its owner's. Now its event's `history` is written empty and the
  thread is read from the channel when the whisper's agent needs it: the
  messages written before the whisper (by their place in the log, not by a
  clock), without any deleted since. This was chosen over scrubbing the
  copies on erase and unsend, because a copy that is never made cannot be
  missed, and the same read works for whispers older releases wrote.
  Their rows (and older team runs' `history`) still hold copies: a row goes
  with any message it quoted, as sent or as edited, and a team run's copy
  leaves the state with it.
- **The deliverable is read from the lanes.** `Mission::deliverable` builds
  it from each released lane's contribution and objective, so erasing a
  contributor, or the requester, takes their words out of it. The
  `DeliverableUpdated` event still records `complete` and wakes the leader,
  but its markdown is written empty. Rows older releases wrote with the
  markdown are deleted with any contributor's erasure and with the ask that
  started the mission.
- **Deleting an ask deletes its mission's copies.** An `@Magnus` message
  deleted for everyone takes the goal and the lane objectives of the
  mission it started (found by its `Asked` record), from the state and the
  log.
- **Erasing a requester clears their goal and objectives.** `MissionLedger::forget`
  follows the content-owner rule: the goal and objectives are the
  requester's. A lane an older release stored under its lane's kind kept
  its objective in the record, where no content row is found for it; the
  erasure names that row in its `Removed`, so a replay clears it too, and
  `migrate` now moves those objectives to the content store, as the
  requester's, as it moved other content written into the log before. The
  log stays append-only: the record cannot be changed by the kernel.

**Reason.** The spec's erasure deletes a person's content wherever it is,
and a delete for everyone takes a message's text out of the log and the
backups. Each copy had escaped because the ledger that held it decided on
its own what erasing meant. One place for the copies, applied through the
same replayed `Removed`, makes the state after a restart match the state
before it.

**Not changed.** A lane's child task holds its objective in its prompt.
For an `@Magnus` ask that task is the asker's own, so it is theirs to
delete; a leader's lane for another member puts the requester's words in
that member's task, which this does not reach.
