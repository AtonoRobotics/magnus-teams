# An update either finishes or is undone

**Choice.**
- Once `UpdateStarted` is recorded, every way out of a switch that does not end in the other release running goes through one owner, `fail_switch` (`update.rs`). It tries every step whatever the one before did: it forgets the pending switch, binds the vault again (for a rollback), records `UpdateFailed` (which ends `updating`) and resumes every task suspended for the update. What it could not undo is logged and said with the reason. A failure record that did not land is made at the next start, as before (`after_start`, "stopped before the switch").
- What may fail before the switch (suspending the tasks, the drain, finding the programs, writing the pending switch, putting the programs in place) is one step, `ready_to_switch`, whose error goes to `fail_switch`. Nothing returns early any more.
- The tasks suspended for an update are resumed in one write.
- A start is marked healthy (`"healthy": true` in `update.json`) by `after_start` once the release serves, before the finish is recorded; the file goes only once the record is made. `on_start` rolls back only a release that started and never served. A healthy one whose finish did not land keeps running, and its next start records the finish.

**Reason.** A store error partway through suspending the tasks, or a vault that would not bind again, returned from the switch early: runs stayed suspended "for update" for good, the server stayed marked as updating, and no failure was recorded. And a finish that could not be recorded left the pending file as it was, so the next start rolled back a release that had come up fine.
