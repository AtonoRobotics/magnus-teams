# #340: list limit, get_task what-it-did, true status enum

## Changed

- `crates/magnus/src/controls.rs`: `list_tasks` honours an optional
  `limit` (default 30, clamp 200); `get_task` adds the
  cannot-be-undone/already-done/possibly-done split from
  `stopped_run_report`.
- `crates/magnus/src/tools.rs`: `list_tasks` spec gains optional `limit`.
- `crates/magnus/instructions/tools.json`: the limit sentence and the
  true status enum (`suspended`, not `waiting`).
- Tests in `tests/scratch/tests/chat_controls.rs`: default list ends at
  30, limit 50 reaches the oldest of 32; a task that wrote memory reports
  `already_done: ["memory write"]`.

## Verification

- Both new tests RED on pristine code, GREEN with the fix.
- Full file 6/8 locally; both taint tests fail pristine-identical on
  this box (noted on #313) and are green in CI.
- `magnus` lib 99/99 (peer's unit test untouched and passing).
