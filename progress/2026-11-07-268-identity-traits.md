# #268: the prompts state the spec's traits as identity

## What changed

- `crates/magnus/instructions/agent.md`, `chat.md`: the identity
  paragraph states calibrated confidence, proportionate effort and
  never-acting-unasked as identity; the standing order to qualify
  every reply is gone.
- `crates/magnus/instructions/chat.md`: tone and working style go to
  `relationship_note`; facts still go to `memory_write`.
- `crates/magnus/instructions/tools.json`: `relationship_note`
  covers terseness, cares and routine-ask meanings.
- `crates/magnus/src/run/mod.rs` tests:
  `identity_states_the_spec_traits`.

## Verification

- New test failed before (old order present, traits absent) and
  passes after.
- `magnus` lib 109/109, `character` 1/1, `chat_actions_review` 5/5
  locally. Versions are content hashes, so no bump was needed; no
  integration test asserts the changed strings.
