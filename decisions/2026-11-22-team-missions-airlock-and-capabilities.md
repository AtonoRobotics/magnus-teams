# Team Missions: Async Fan-Out/Fan-In, Release Airlock, and Member Capabilities (Phase 1)

**Choice.** Introduce the Phase 1 backend foundation for the Teams redesign:

1. **Rollback-safe event log (`crates/magnus/src/events/mission.rs`, `Ev::TeamMission`):**
   - `TeamMissionEvent` records `Started`, `LaneDispatched`, `LaneDraftReady`, `LaneReleased`, `LaneFailed`, `DeliverableUpdated`, `StandingReleaseSet`, `AvatarSet`, and `AppVisibilitySet` under a single top-level `Ev::TeamMission` kind (older binaries skip unknown top-level kinds cleanly on rollback).
2. **Strict per-user Airlock ledger (`crates/magnus/src/team/missions.rs`):**
   - `MissionLedger` lives in `State.missions` and implements `Forget` (`forget_person` erasure cascade).
   - `view_for_room(ws, room, viewer)` strictly strips `private_draft` unless `lane.owner == Some(viewer)`.
3. **Async Fan-Out / Fan-In Orchestrator (`crates/magnus/src/team/orchestrator.rs`):**
   - Replaces synchronous 5-minute polling and 40-message whisper dumps with detached lane watchers that dispatch clean, scrubbed objectives to isolated `MemberAgent`, `OpenWeb`, and `PeerAgent` lanes.
   - Implements the One-Sentence Rule: a person's own request in a Room (`target == requester`) or an active `StandingRelease` auto-releases scrubbed results directly; cross-user requests hold privately in the target member's Release Airlock until they click Release, approve in chat, or save a standing rule.
4. **HTTP API & Capability Discovery (`crates/magnus/src/team/mission_routes.rs`):**
   - `GET/POST /v1/workspaces/{ws}/rooms/{room}/missions`
   - `POST /v1/workspaces/{ws}/missions/{mission}/lanes/{lane}/release` & `steer` (owner-only, `scrub_check` enforced)
   - `GET /v1/workspaces/{ws}/capabilities` (member app names + `ProfileAvatar`, respecting `hidden_apps` toggles; zero credentials or private data)
   - `PUT /v1/profile/avatar` & `PUT /v1/profile/team-apps`.

**Reason.** Eliminates synchronous blocking, cross-user privacy leaks, and double-confirmation ceremony while laying the clean foundation for Phase 2 (unified workspace RLS memory) and Phase 3 (Obsidian Aurora Shared Rooms + Video-ready Work Canvas UI).
