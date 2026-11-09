# Completion reviewer sees the run's evidence (#384)

## What changed

- `run/reviewer.rs`: the completion prompt gains the run's record
  (`results_in_order`, clipped), so holds judge the work, not the
  report.
- `tests/scratch/tests/reviewer.rs`:
  `the_completion_reviewer_sees_the_runs_tool_results`: an overstated
  report stays for the person, and the reviewer request carries the
  tool results. Red without (no results in the prompt), green with.

## Verification

- reviewer: 5 passed. modules: 4 passed. lib: 113 passed.
