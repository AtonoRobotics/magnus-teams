# #270: each run is told its harness as it is

## Changed

- `crates/magnus/src/run/mod.rs`: `harness_text` builder plus the
  `harness_context` hook in system assembly, between instructions
  and person sections.
- `crates/magnus/instructions/agent.md`, `chat.md`: the web-search
  and sandbox promises conditioned on the harness section.
- Tests: `harness_names_only_what_is_offered` unit test;
  `tests/scratch/tests/harness_description.rs` captures both runs'
  system prompts with and without search/sandbox.

## Verification

- New e2e RED on pristine code (no harness section, old promises),
  GREEN with it; unit test likewise.
- `magnus` lib 100/100, `character` green, the inspector's
  instruction-version test green; clippy clean on touched regions.
