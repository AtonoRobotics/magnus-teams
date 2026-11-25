# 2026-11-25 — Context and Background Token Limits

## Choice
Build on `2026-11-25-cached-prompts-and-no-polling.md` to further cut token burn across all models:

1. **Anthropic tool-catalog cache breakpoint (`crates/adapter-anthropic/src/lib.rs`)**: Mark the last tool spec with `cache_control: { "type": "ephemeral" }` so the ~6.5k-token tool catalog caches independently even when per-user/per-run system context changes.
2. **Tighter per-result and screenshot context limits (`crates/magnus/src/run/mod.rs`)**:
   - Lower `MAX_RESULT_IN_CONTEXT` from 40,000 chars (~10k tokens per tool output) to 16,000 chars (~4k tokens; full outputs remain in the task log).
   - Retain only the latest 2 tool/frame screenshots in `msgs[1..]` (`MAX_IMAGES_IN_CONTEXT = 2`), replacing older screenshots with `[earlier screenshot omitted]` while preserving user-attached images in `msgs[0]`.
3. **Lightweight background passes (`crates/magnus/src/run/memory.rs`, `crates/magnus/src/run/reviewer.rs`)**:
   - Route background `memory_pass` calls to `[model] worker` when configured (falling back to the run's model) and cap output to `MEMORY_PASS_MAX_TOKENS = 1024`.
   - Cap `reviewer_call_within` output to `REVIEWER_MAX_TOKENS = 2048`.

## Reason
Even with prompt caching and parallel tool calls enabled, oversized tool outputs (up to 40k chars each), unbounded historical screenshots in browser/computer loops, and 16k-token background `memory_pass`/`reviewer` calls on the primary model still inflated token spend across all providers. These caps reduce token volume on every model without changing security boundaries or event kinds.
