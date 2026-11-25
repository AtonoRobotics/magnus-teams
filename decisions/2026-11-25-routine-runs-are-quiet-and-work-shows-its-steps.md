# A schedule's routine runs are quiet, and work shows its steps

## Decision
1. A schedule's first run is kept in the outcomes ledger as the log records
   it, not looked up among the tasks still listed. Before this, deleting a
   heartbeat's earlier runs made the next one its "first" again, so it sent
   "Finished: Heartbeat" as a notice. Production showed that on 2026-11-24:
   the heartbeat was resumed after its old runs were deleted, and its next
   run sent a notice.
2. A task row says `routine` when it is a schedule's run after its first
   that does not need its person: it has not failed, and waits on no one
   but a timer (`outcomes::routine`). The workspace shows no box for such a
   run, neither while it works nor after, and "Since you were away" does not
   count it. It stays in Tasks.
3. The workspace no longer has an "Up Next" box. A run due hours from now is
   not something to look at now, and Schedules shows when each one runs.
4. A task row carries `recent_steps`, its last four actions newest last,
   and `actions`, how many it took. Each step is its tool's plain words
   (`doing_words`, from the tool's name only) with how it went: done,
   failed, running or stopped. Run records (a plan, a note) are left out. A
   task's box lists them while it works, and once it is done says how many
   actions it took and how many receipts the server checked.
5. A task opened while it is still running opens on Steps. Its result is
   not there yet.

## Reason
The owner, 2026-11-24: the heartbeat "should be silent", yet each run left
a notice and the workspace pointed at a run 17 hours away, "a waste of
attention". Their tasks "just sit there with the same thing and switch to
done once complete, never showing the user any evidence or traces." This
follows the rule that a repeating schedule shows only the first time, when
its person is needed, and when it finishes for good (owner, 2026-11-16), and
the standing rule that notices are only for finished work or a decision
needed. Steps carry only tool names, never the words a call carried, so
nothing a page or message wrote reaches a box without the person opening
the task.

## Also
5c4779e left main's fast tier red, so nothing has been released since
c5b830b. It changed the harness's web-search sentence and added `callout`
and `sources` to the elements `show` and `task_complete` offer, but did not
update the harness test or the tool-spec pin. Both are updated to match:
the change was deliberate and recorded in
`2026-11-24-faster-research-and-compose-validation.md`.
