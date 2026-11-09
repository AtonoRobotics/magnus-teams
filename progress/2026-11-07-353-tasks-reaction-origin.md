# #353: task-list reactions follow their answer, scheduled or not

## What changed

- `crates/magnus/src/reactions.rs`: the `t:` branch of
  `reactions_before` looks up runs first, then tasks, and takes the
  originating answer (`a:{run.task}`) as its visibility origin,
  mirroring `origin_turn`.
- `crates/magnus/src/reactions.rs` tests:
  `task_list_reactions_follow_their_originating_answer`.

## Verification

- New test failed before (the scheduled reaction dropped) and passes
  after; hiding the originating answer now drops its reaction too.
- `magnus` lib 108/108, `reactions_disagreements` 7/7 locally; no new
  fmt diffs, no new clippy lints.
