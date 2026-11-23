# The mission ledger owns lane transitions

**Choice.** One table, `LaneStatus::next(step)` in `team/missions.rs`, says what a mission lane can do next. The steps are `Draft`, `AutoRelease`, `OwnerRelease` and `Fail`:

| from \ step     | Draft          | AutoRelease | OwnerRelease | Fail   |
|-----------------|----------------|-------------|--------------|--------|
| Working         | WaitingRelease | Released    | no           | Failed |
| WaitingRelease  | WaitingRelease | no          | Released     | no     |
| Failed          | WaitingRelease | no          | Released     | no     |
| Released        | no             | no          | no           | no     |

`MissionLedger::apply` applies a recorded lane event only when the table admits it (a recorded release does not say which release it was, so either one counts). Every writer of a lane event goes through `Kernel::move_lane` (`team/lanes.rs`), which checks the table and writes the event under one lock: the release route, the steer, the watcher and the boot. The caller posts to the room and refreshes the deliverable only when the event was written. The route's own `RELEASING` lock is gone; `move_lane`'s lock covers the watcher as well.

What this fixes:

1. The owner could release a lane still `Working`. That raced the watcher's own release: the ledger dropped the second `LaneReleased`, but the room post and the deliverable refresh still ran. The owner now releases only a draft (or, as before, their own words on a failed lane). A lane still working is answered 404, and a release that loses the race 409.
2. A `LaneFailed` overwrote a `Released` lane. Only a working lane fails now. A failure recorded against a waiting draft (an old steer race) no longer discards it either. These are the only replays that change.
3. Watchers lived only in the process that dispatched them, so a restart left every working lane working for ever. Worse, a dispatched lane was never read back at all. `LaneDispatched` wrote its lane's kind as `kind`, the key that names the event's own kind, so its row was stored as a `member_agent`, `open_web` or `peer_agent` event and skipped at every boot. On the wire the lane's kind is now `lane_kind`. `events::current_lane_dispatched`, called by `log::parse_event`, reads old rows in the current form, so an event kind is still never edited. The boot (`Kernel::resume_lanes`) attaches a watcher again to each working lane whose run is still live. It fails a lane whose run ended while the server was down, whose run is gone, or that never had one. A result from before the restart is not posted into the room late, since it may be weeks old by then. `LaneDispatched` also records its `thread` (`serde(default)`, not written when empty), so a lane followed after a restart posts in the thread it was asked in. A watcher stops once its lane has left `Working` (after a steer's draft, say).
4. An open-web lane has no owner, so its release was credited to `open_web`, which is no one. Erasing the requester missed the release, its workspace-memory entry and its room post. `MissionLedger::release_credit` credits the mission's requester. For old logs, `content_owner` and `Forget` read `open_web` as the requester, and `team::lanes::OPEN_WEB_OWNERS` re-owns old content rows: the release, and the room message with the same text posted after it. It runs after `CONTENT_OWNERS` at `migrate` and after a restore.

An open-web lane whose result fails the scrub has no Airlock to wait in. It used to stay working for ever, and now it fails.

**Reason.** Before, four writers each kept their own idea of which lane may do what: the route checked "not released", apply checked nothing on failure, and the watcher checked nothing. When the transitions live in one table that every writer and the replay consult, none of these bugs can come back by one writer drifting from the others. The boot rule takes the safer reading of "ended while the server was down": fail the lane rather than post a stale result. The owner can ask again.

**Not done.** Old `LaneDispatched` rows were stored under a kind with no content fields, so their objective (the requester's words) sits in the event record rather than the content store. Erasing the requester does not remove it. Moving it needs a migration of stored rows, so it is left for the owner to decide.
