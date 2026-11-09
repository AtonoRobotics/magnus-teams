# #347: schedule edits are authorized as edits

## What changed

- `crates/magnus/src/controls.rs`: `edit_schedule` passes
  `"edit_schedule"` to `schedule_owned` (was `"cancel_schedule"`).
- `tests/scratch/tests/chat_controls.rs`:
  `an_org_policy_tells_schedule_edits_from_cancels` plus a
  `schedule_set` helper; forbidding each op stops it and leaves the
  other.

## Verification

- New test failed before (the edit went through under the edit-forbid)
  and passes after.
- `chat_controls` 10/10, `magnus` lib 103/103 locally.
