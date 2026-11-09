# #330: read aloud at the call cap reads the answer itself

## What changed

- `crates/magnus/src/chat_actions.rs`: `write_spoken` falls back to the
  answer text once the run is at its model-call cap. No grant check
  (see decisions file).
- `tests/scratch/tests/composed.rs`: a chat turn under
  `max_model_calls(1)`; read aloud returns the answer unwritten.

## Verification

- New test failed before (a script written past the cap) and passes
  after.
- `composed` 6/6, `aux_budget` 4/4, `chat_actions` 5/5, `magnus` lib
  92/92 locally; no new clippy lints.
