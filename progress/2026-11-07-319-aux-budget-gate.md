# #319: auxiliary model calls are worst-case gated on the budget

## What changed

- `crates/magnus/src/boundary.rs`: new `Kernel::aux_max_tokens` (full
  allowance or skip, never suspend); `clamp_output_for_budget` and
  `estimated_input_tokens` moved here from `agent_loop` with the clamp
  unit test.
- `run/verify.rs`, `run/memory.rs`, `knowledge.rs`, `chat_actions.rs`:
  each auxiliary call gated; spoken falls back to the answer text.
- `tests/scratch/tests/aux_budget.rs`: a spend-reporting chat fake and
  four tests, one per site.

## Verification

- All four tests failed before the fix (aux went out on ~$0.10 left) and
  pass after; each RED was checked to fail on the behavior, not setup.
- Funded paths still run: `model_retries` 5/5, `knowledge` 11/11,
  `composed` 5/5, `memory` 3/3, `modules` 3/3 (funded verifier),
  `chat_actions` 5/5, `magnus` lib 90/90; no new clippy lints.
- `outcome`, `contract_freeze`, `relationship_memory` fail on this box
  identically with a pristine kernel (box-load disease, same as #313);
  noted on #313.

## Test-design notes

- Spend sits at ~$0.90 of $1: the boundary suspends model actions at
  full spend, so the run must stay under while no aux worst case fits.
- The fake matches aux calls by system-prompt substring: the loop offers
  `memory_write` too, so offered tools do not tell them apart.
- Chat tool-call ids must be unique across replies, or the kernel
  replays the first call.
