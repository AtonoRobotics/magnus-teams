# #358: control `turn` is the chat turn's task

## Changed

- `crates/magnus/src/boundary.rs`: the control-tool dispatch passes the
  asking run's task (the chat turn) to `chat_control` instead of the run.
- `crates/magnus/src/controls.rs`: `chat_control`'s parameter renamed
  `run` to `turn`, with a doc line; `sources` names its value `turn`.
- Test: `a_cancelled_runs_log_says_button_or_conversation` pins
  `turn == chat["task_id"]` for the cancel and the pause (was `run_id`).

## Verification

- The updated test RED on pristine code (turn held `run_…`), GREEN with
  the fix.
- `chat_controls` 9/11 locally; the 2 taint failures are
  pristine-identical on this box (verified by stash, noted on #313).
- Magnus lib 108/108 locally.
