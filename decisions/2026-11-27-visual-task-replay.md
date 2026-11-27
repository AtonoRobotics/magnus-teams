# Visual Task Replay with step-indexed screenshots and decisions

**Date:** 2026-11-27

## Decision
1. **Timeline frames (`crates/magnus/src/api/work.rs`):** Expose `run.frames` as `"frames": [{ "step", "image_b64" }]` in `timeline_of(run, redact_private)` when `redact_private` is `false` (empty when `redact_private` is `true` so private team redaction stays intact).
2. **Visual Replay module (`desktop/ui/task-replay.js`, registered in `index.html` and `assets.rs`):** Upgrade Nova's step scrubber (`nova-ux.js`) into an interactive replay stage with `←` / `Play` / `→` playback controls, synchronized VM screenshot frames (`frameForStep`), tool call inputs/outputs, and boundary decision pills (`allow` / `ask` / `block`).

## Reason
The previous Task Replay bar only toggled opacity classes on `.step` text elements. Exposing the screenshots `Run` already captures in `run.frames` turns Task Replay into a genuine visual timeline paired with Magnus's server-verified boundary decisions. Verified by `desktop/tests/task-replay-check.mjs`.
