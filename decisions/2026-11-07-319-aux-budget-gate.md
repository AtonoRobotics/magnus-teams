# Gate the auxiliary model calls on the budget (#319)

The worst-case gate (#273) covered the agent loop's calls. The four
auxiliary calls went out with the run's full allowance no matter what was
left: verifier judgments, the memory pass, knowledge extraction, spoken
scripts. Each could overshoot a nearly spent budget.

All four now go through one `Kernel::aux_max_tokens` in the boundary: the
adapter measures the prompt (#323), the quote prices it, and the loop's
clamp helper (moved here, since `agent_loop` is private to `run/`)
decides. Full allowance or nothing: auxiliary calls are optional extras,
so they run whole or not at all — a truncated judgment is worse than
none, and the run is never suspended for one. Skipped verifier and memory
calls return none; a skipped extraction reads nothing; read aloud falls
back to the answer itself, which is what is heard.

Two fallbacks differ from the loop deliberately. An estimate the adapter
cannot give falls back to the kernel's bytes (a third-party adapter
without the op keeps working). A quote it cannot give skips (the loop
asks anyway: its work must proceed, and the call fails through retries;
an auxiliary call would only fail — read aloud with a 500).

Observed gap, not taken: read aloud checks neither the grant nor the
model-call cap, unlike the other three sites. Filed separately.
