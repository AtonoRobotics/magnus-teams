# #271: a task from an accepted card is told it was approved

## What changed

- `crates/magnus/src/run/mod.rs`: new `scope_line` helper; the context
  tells schedule runs the confirmed-schedule wording and one-off tasks
  the approved-task wording, keyed off `task.schedule`.
- `tests/scratch/tests/suggestion_cards.rs`: accepting an untimed
  suggestion starts a task whose first request carries the approved
  wording and not the schedule one.

## Verification

- New appliance test failed before (the wrong sentence, quoted in the
  failure) and passes after; unit test pins both wordings.
- `suggestion_cards` 8/8, `task_proposals` 4/4, `magnus` lib 93/93
  locally; no new clippy lints.
