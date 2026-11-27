# 2026-11-27 — Rich Visual Task Replay Metadata (Tier 5 Phase 1)

## Choice
- Enhanced [`desktop/ui/task-replay.js`](file:///home/antigravity/AtonoRobotics/magnus-teams/desktop/ui/task-replay.js) to surface the full per-step tool input (`summarizeInput()` + collapsible `Input details` JSON) and verification count badges (`✓ N verified` from `/v1/tasks/:id/timeline` `verifications`) alongside VM screenshots and tool outputs.

## Reason
- `crates/magnus/src/api/work.rs` `timeline_of()` already returns redacted tool inputs and verification records; surfacing them directly in the replay stage lets users inspect both what Magnus *sent* and what was *verified* at every scrubbed step.
