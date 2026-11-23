//! Event payloads for Team Missions, async Fan-Out/Fan-In collaborator lanes,
//! the Release Airlock, member capability visibility, and user avatars.
//!
//! Carried by a single top-level `Ev::TeamMission` variant so older binaries
//! skip unknown top-level kinds cleanly on rollback (`log.rs`).

use serde::{Deserialize, Serialize};

/// Which kind of isolated worker executes a Mission lane.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LaneKind {
    MemberAgent,
    OpenWeb,
    PeerAgent,
}

/// User identity display preference across Rooms, chat, People rail, and calls.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileAvatar {
    #[default]
    Default,
    Preset {
        id: String,
    },
    Photo {
        blob_id: String,
    },
}

/// Append-only event payloads for Team Missions and user collaboration settings.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum TeamMissionEvent {
    Started {
        workspace: String,
        room: String,
        mission: String,
        #[serde(default)]
        goal: String,
        requester: String,
        #[serde(default)]
        leader_run: Option<String>,
    },
    LaneDispatched {
        workspace: String,
        mission: String,
        lane: String,
        kind: LaneKind,
        #[serde(default)]
        target_member: Option<String>,
        #[serde(default)]
        objective: String,
        #[serde(default)]
        scope: String,
        #[serde(default)]
        auto_release: bool,
        #[serde(default)]
        child_task: Option<String>,
        #[serde(default)]
        child_run: Option<String>,
    },
    LaneDraftReady {
        workspace: String,
        mission: String,
        lane: String,
        owner: String,
        #[serde(default)]
        draft: String,
        #[serde(default)]
        scrub_flagged: bool,
    },
    LaneReleased {
        workspace: String,
        mission: String,
        lane: String,
        #[serde(default)]
        contribution: String,
        released_by: String,
    },
    LaneFailed {
        workspace: String,
        mission: String,
        lane: String,
        reason: String,
    },
    DeliverableUpdated {
        workspace: String,
        mission: String,
        #[serde(default)]
        markdown: String,
        #[serde(default)]
        complete: bool,
    },
    StandingReleaseSet {
        workspace: String,
        room: String,
        person: String,
        scope: String,
        active: bool,
    },
    AvatarSet {
        person: String,
        avatar: ProfileAvatar,
    },
    AppVisibilitySet {
        person: String,
        app: String,
        hidden: bool,
    },
}
