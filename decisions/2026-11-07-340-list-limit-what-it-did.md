# #340: list limit, get_task what-it-did, true status enum

The remainder of #265 after #338 landed get-by-id in parallel (PR #339
closed as superseded): discovery past the newest 30, what a running task
did, and the status vocabulary.

## Choices

- `list_tasks` takes an optional `limit` (default 30, clamped to 200),
  keeping the token-cheap default while making older tasks discoverable
  again. Get-by-id alone cannot do that: it needs an id from somewhere.
- `get_task` adds the cancel report's split (cannot-be-undone first) via
  the existing `stopped_run_report`, so a task with no summary yet still
  says what its run did. The peer's `controlled_task` is untouched, and
  its unit test with it.
- The descriptions said `waiting`, which `status_str` never returns; they
  now say `suspended`.
