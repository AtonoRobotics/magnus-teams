# 2026-11-27: Retire Missions from Magnus Projects (Phase 1)

## Choice
Stop starting Missions when people chat with `@Magnus` in a shared project (or when routines / quiet-thread follow-ups fire), and remove all Mission / Release Airlock / Start-shared-work UI surfaces:

1. **Direct personal-agent room replies (`crates/magnus/src/team/reply.rs`)**:
   - `post_and_ask`, `routines.rs`, and `conversation.rs::follow_up_quiet_threads` now call `Kernel::reply_in_room` / `Kernel::reply_in_room_as` instead of `start_personal_room_mission` / `start_room_mission_as`.
   - Each ask runs as the asker's own personal Magnus (`Label { untrusted, private: true }`) linked to the channel via `Change::Whisper`—emitting zero `TeamMissionEvent`s and returning no `mission_id`.
   - On completion, the reply passes the existing outbound boundary gate (`lane_release::checked_send`: scrubber, taint, outgoing classifier, Auto Review). Clean replies post directly to the conversation; held replies stay private to the asker inline in the chat with `held_because`, `Share`, and `Not now`.
   - `State::background_asks` checks `team::Ledger::is_routine_task(task)` (recorded on `Change::RoutineRan` before wake) alongside legacy `MissionLedger::is_routine_lane`.
   - `copies_of_message` includes direct `w.chat.asked` tasks so unsending an `@Magnus` message still erases its task copy.
2. **Mission UI removed**:
   - Deleted `desktop/ui/team-rooms.js` (removed from `desktop/ui/index.html` and `crates/magnus/src/ui/assets.rs`) and its 22 unused CSS rules in `desktop/ui/teams.css`.
   - Removed Mission / Release Airlock Bento tiles, the `Missions` Views entry, and the `Start shared work` form from `desktop/ui/shared-canvas.js` and `desktop/ui/team-panel.js`.
   - Stopped polling `/v1/workspaces/{ws}/rooms/{room}/missions` in `desktop/ui/teams.js`.
   - Pointed the shared conversation header breadcrumb in `desktop/ui/team-conversation.js` to `shared:files` instead of `shared:missions`, and surfaced `held_because` inline on held private replies.
3. **Replay preserved**:
   - `Ev::TeamMission` and `MissionLedger` remain intact for replaying and forgetting historical logs.

## Reason
Projects should work like private chat plus shared documents and shared chats. Multi-lane Missions, Living Deliverables, and a separate Missions/Airlock panel were drift that turned ordinary shared conversations into heavy orchestration flows. Running each ask directly as the asker's personal agent behind the existing outbound boundary gate preserves the full privacy boundary while removing the Mission layer.

## Invariant tests
`tests/invariants/tests/inv8_team.rs` asserts `mission_id` and `/rooms/{channel}/missions` on shared chat asks (lines 81–118). A patch (`phase1-retire-missions.invariants.patch`) is provided for the owner to apply to `tests/invariants/`.
