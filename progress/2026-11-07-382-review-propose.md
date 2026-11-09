# Reviews surface tasks as observations, not proposals (#382)

## What changed

- `instructions/review.md`: the propose line splits into automations
  (`propose_schedule`) and surfacing tasks (`observe`); six decisions.
- `run/mod.rs` tests: `review_names_only_tools_a_review_can_use`
  pins that reviews are never told about `propose_task`.

## Verification

- New test red without the doc fix, green with it. lib: 114 passed.
  proactive e2e: 6 passed (observe path already covered there).
