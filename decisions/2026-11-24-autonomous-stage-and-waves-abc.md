# 2026-11-24 — Autonomous self-composing stage and Waves A, B, and C

## Context
Users should not have to learn menus or hunt across panels to operate an autonomous worker. Top-bar app dropdowns duplicated navigation, the right sidebar competed with live work, and several backend capabilities (pausing tasks, editing before approving, staged drafts, calendar-grade recurrences, trust graduation, ambient triage, deterministic undo, and briefings) were either unwired or missing.

## Decisions

### Milestone 0 — Autonomous self-composing stage and on-demand right sidebar
- **Choice:** Remove the top-bar `.stage-dock-more` dropdown completely and keep a single `[data-toggle-inspector]` toggle in `convHead` (plus the compact Notification Center / DND bell). Keep the right sidebar strictly on-demand for looking up current or past details, while live tasks, sub-agents, approvals, and deliverables automatically compose the active conversation stage (`section.conv[data-stage-mode="composed"]`).
- **Reason:** Eliminates redundant chrome and lets active processes surface and rearrange the stage automatically without forcing the user to manage panels.

### Wave A — Quick wins & truthful controls
- **Choice:**
  - Fix overdue commitment wakeups using a named horizon constant (`COMMITMENT_WAKE_HORIZON_HOURS`) and include recalled `what`/`to` fields in proactive wakes and `fact`/`topic` in `memory_summary_json`.
  - Allow re-attaching previously delivered files (`library.rs`), running any schedule immediately (`POST /v1/schedules/{id}/run`), pausing running tasks (`POST /v1/tasks/{id}/pause`), writing memory facts (`POST /v1/memory`), and listing/deleting playbook lessons (`GET /v1/playbook`, `DELETE /v1/playbook/{id}`).
  - Add deck PPTX/PDF export, table Copy/CSV export, HTML fullscreen, deliverable Markdown Copy/Export/Fullscreen, Personal Spend Meter, dedicated `Memory & Promises` Settings tab, starter-card app readiness badges, and truthful onboarding copy (removing the unused Discovery Safety Mode selector).
- **Reason:** Closes high-friction daily gaps using existing kernel primitives without changing event schemas.

### Wave B — Approvals, progressive trust & calendar-grade routines
- **Choice:**
  - Encode calendar-grade recurrences (`weekdays HH:MM`, `weekly mon,fri HH:MM`, `monthly 15 HH:MM`, `once YYYY-MM-DDTHH:MM`) backward-compatibly in `Schedule::daily_at` while accepting structured JSON fields (`weekdays_at`, `weekly_at`, `monthly_at`, `once_at`) in the API.
  - Support `@domain.com` Cedar wildcard rules (`context.destination like "*@domain.com"`) and Trust Graduation suggestions (`GET /v1/rules/suggestions`, ≥3 non-financial approvals).
  - Support edit-before-approving (`Kernel::approved_edits`), real Staged Draft execution through `Kernel::execute`, batch inbox decisions (`POST /v1/inbox/batch`), and non-mutating schedule `dry_run` previews.
- **Reason:** Encoding extended recurrence rules in `daily_at` keeps `Ev::ScheduleSet` and existing snapshots untouched while delivering calendar-grade routines; edit-before-approve and progressive trust reduce approval fatigue without weakening Cedar guardrails.

### Wave C — Executive briefing, performance, intelligence & polish
- **Choice:**
  - Add deterministic ambient-watch triage (`triage_watched_diff`) before LLM reviews so `Ignore`-urgency diffs advance `WatchedSeen` at zero token cost.
  - Increase live tool cache freshness (`LIVE_TOOLS_FRESH` 2s → 30s), parallelize consecutive non-record `Risk::Read` tool calls and connector tool discovery via `join_all`, and inject up to 5 recent completed project tasks into `context_with()`.
  - Wire `memory_index::hybrid_score()` into recall and `memory_index::is_near_duplicate()` into `Op::MemoryWrite`.
  - Add `GET /v1/briefing`, `POST /v1/tasks/{id}/undo/{call_id}`, deck `"table"` slide formatting + WinAnsi Unicode PDF encoding, and Linear + Notion remote DCR MCP catalog manifests.
  - Add Morning Briefing card in Activity, Notification Center & DND snooze popover, explicit `"Steering active task"` composer pill, confirmation before destructive message edit-and-resend, task search/status filters/bulk clear, block-level `"Edit with Magnus"`, `"Ask Magnus"` on files, inline project rename, and sidebar conversation quick delete.
- **Reason:** Cuts redundant latency and token spend, makes executive state visible in one glance, and removes everyday friction while staying within all file-size, clippy, contrast, and zero-event-breakage constraints.
