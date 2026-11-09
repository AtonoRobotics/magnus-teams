# #273: worst-case budget gate on every loop call

New adapter-proto `quote` (both adapters), `Adapters::quote`, and a per-attempt clamp in the agent loop: full output when it fits, shortened to what is left, budget stop with notice when the prompt alone does not fit.

RED: `a_budget_one_call_would_exceed_asks_the_model_nothing` (3 calls, $0.0012 past a $0.001 budget unfixed); `what_is_left_bounds_the_call_output` (unclamped 16000 unfixed). GREEN: both pass; `model_retries.rs` 4/4; `magnus` lib 88/88 including the clamp-arithmetic unit test.
