# Heartbeat as a quiet system task and auto-clearing routine notifications

**Choice.**
1. Heartbeat is a system schedule (`Ev::ScheduleSet { system: true }`, `Schedule.system = true`, `RunConfig.modules["system_task"] = true`, with backwards-compatible detection for existing Heartbeat schedules). System schedules never attach to a chat thread (`schedule_thread::of` returns `None`), never emit `task_ended` notifications or chat entries on normal completion (not even on their first run), and stay hidden from chat, stage bento tiles, rail badges, and task lists unless the user is actually needed (an approval/question/review or a failure).
2. Routines no longer leave lingering notifications after setup:
   - Threadless routine runs stay silent on normal completion (notifying only on failure, human-in-the-loop waits, or `ScheduleEnded`).
   - Accepting or dismissing the last open `suggestion` from an Explore ideas run atomically resolves its open `"ideas"` notice (`inbox.rs`).
   - Resuming, editing, stopping, or deleting a schedule atomically dismisses open `schedule_*` / `paused_by_admin` notices for that schedule (`controls.rs`).
   - `Notices.syncInbox()` prunes resolved inbox items and stale `"ideas"` notices from `Notices.history`, live `#notices` popups, `AuroraStage.notes`, and refreshes the header bell badge (`convHead`).
   - Explore's routine-scheduled confirmation message auto-clears after 4 seconds.

**Reason.** Owner direction (2026-11-24): Heartbeat is a background system check that should never pollute chat or notify unless the user is needed, and routine setup/status notifications should disappear once handled instead of lingering.
