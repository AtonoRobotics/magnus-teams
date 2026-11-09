# #393: reviews propose tasks to the inbox

Reviews can `propose_task` now. From a chat turn the card still goes
in the thread (#254, untouched); from a review it lands in the inbox
as `task_proposal`, grounded and budgeted as a chat card is. Accept
starts the task with the approved cost; reject ends it quietly.

Changed:

- `crates/magnus/src/suggestions.rs`: `propose_task` validation
  extracted into `proposal_parts`, shared by the thread-card path and
  a new `inbox_proposal` for runs with no thread.
- `crates/magnus/src/boundary.rs`: `propose_task` is no longer
  chat-only at the gate.
- `crates/magnus/src/run/mod.rs`: reviews are offered
  `propose_task`; the #382 test now asserts reviews can propose.
- `crates/magnus/instructions/review.md`: the "surface as an
  observation" line is a `propose_task` line.
- `crates/magnus/instructions/tools.json`: the tool's description
  covers the inbox from a review.
- `crates/magnus/src/inbox.rs`: `task_proposal` accept starts the
  task (card budget within the appliance's, proposing run's model
  and taint, woken at once) and reject resolves quietly.
- `desktop/ui/app.js`: the inbox renders the proposal card with its
  evidence and Start/Dismiss.

Tests: `a_review_proposes_a_task_the_person_approves`
(`tests/scratch/tests/proactive.rs`) fails without the change (no
proposals land) and passes with it: two proposals arrive grounded
and budgeted, accept starts the task at the approved cost, reject
leaves nothing behind. Suites green locally: `magnus --lib` 123,
`proactive` 10, `task_proposals` 4, `cold_start_proposals` 5,
`standing_authority` 7; `node --check` on the desktop file.
