# #267: cancels and pauses say how; every Magnus message takes reactions

## What changed

- `crates/magnus/src/events.rs`: `source`/`turn` on `Cancelled`,
  `SchedulePaused`, `ScheduleResumed`, `ScheduleChanged`,
  `ScheduleRemoved` (all defaulted, old logs read on).
- `crates/magnus/src/controls.rs`: the five control methods take the
  turn; `sources()` maps it to conversation-plus-run or button;
  `chat_control` takes the run.
- `crates/magnus/src/api.rs`, `boundary.rs`, `chat_actions.rs`,
  `person_admin.rs`, `kernel.rs`: button, run, button, console and
  automatic at each emit site; `state.rs` patterns take `..` (the
  projection does not keep the source).
- `crates/magnus/src/reactions.rs`: `MAGNUS` gains `tasks` and the six
  card kinds; `reactions_before` resolves `t:` entries to their run
  and `c:` entries to the proposing run via a lazily built card index.
- Tests: `chat_controls.rs`
  `a_cancelled_runs_log_says_button_or_conversation`,
  `reactions_disagreements.rs`
  `started_tasks_and_cards_take_reactions`, `controls.rs`
  `a_controls_event_names_conversation_or_button`, `reactions.rs`
  `reactions_on_tasks_and_cards_read_their_run`.

## Verification

- New tests failed before (no source/turn; 400s on tasks and cards)
  and pass after.
- `magnus` lib 105/105, `chat_controls` 11/11,
  `reactions_disagreements` 7/7, `app` 17/17, `cancel_report` 2/2,
  `task_policy` 1/1 locally; touched lines fmt-clean (the `api.rs`
  drift below line 1100 is pre-existing).
