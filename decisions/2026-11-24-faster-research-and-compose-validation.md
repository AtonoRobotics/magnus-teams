# Faster research workers, optional worker model, and compose validation for callout/sources

**Date:** 2026-11-24

## Decision
1. **Fix `compose::validate` for `callout` and `sources`** (`crates/magnus/src/compose.rs`):
   - Add `"callout"` and `"sources"` to `KINDS`, `SHOWN`, and `validate()` so `magnus-ui` blocks using those elements pass `ui::docs::problems()` without triggering retries.
2. **Reduce model calls and latency on web research**:
   - Add optional `[model] worker = "adapter/model"` in `crates/magnus/src/config.rs` (`Model::worker_model`) so `research_web` workers can run on a fast/cheap model while the parent synthesizes on the main model.
   - Cap `research_web` workers to `RESEARCH_MAX_MODEL_CALLS = 8` and `RESEARCH_MAX_ACTIONS = 12` (`crates/magnus/src/compartment/mod.rs`), clamped within the parent's remaining budget, and tighten the worker prompt to batch searches and inspect at most 2–3 authoritative pages.
   - Update `crates/magnus/src/run/mod.rs` so simple public fact lookups without private taint use direct `web_search` instead of spawning a full `research_web` sub-agent.

## Reason
`callout` and `sources` were added to `ui::catalog` but missing from `compose::validate`, causing Magnus to fail validation and retry when emitting them. Meanwhile, `research_web` workers inherited up to 75 model calls on the parent's heavy model and were preferred even for simple public lookups, causing 20+ model calls and multi-minute latency on straightforward comparisons.
