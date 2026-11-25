//! Event payloads for Team Missions, async Fan-Out/Fan-In collaborator lanes,
//! the Release Airlock, member capability visibility, and user avatars.
//!
//! Carried by a single top-level `Ev::TeamMission` variant so older binaries
//! skip unknown top-level kinds cleanly on rollback (`log.rs`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
        /// `lane_kind` on the wire: `kind` names the event's own kind.
        #[serde(rename = "lane_kind")]
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
        /// The thread its result is posted to, so a watcher attached again
        /// after a restart posts where the request was made.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thread: Option<String>,
        /// The routine this lane fires for: background work that asks no
        /// one (`State::background_asks`). Recorded before its run wakes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        routine: Option<String>,
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
        /// Why a lane that would have posted itself waits for its owner
        /// instead: a check held it (`team::lane_release`). Shown to the
        /// owner only. Absent from events written before 2026-11-23.
        #[serde(default)]
        held_because: Option<String>,
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

/// Before 2026-11-22 a `LaneDispatched` wrote its lane's kind as `kind`, the
/// key that names an event's kind, so its row was stored as a
/// `member_agent`, `open_web` or `peer_agent` event, a kind no release
/// knows, and every boot skipped it: after a restart the lane was gone.
/// Such a row in its current form, the lane's kind under `lane_kind`.
/// Its objective was kept in the record, as no content field of those
/// kinds was listed; `log::migrate` moves it to the content store, where
/// erasure deletes it (`events::content_fields`).
pub fn current_lane_dispatched(body: &Value) -> Option<Value> {
    let kind = body.get("kind")?.as_str()?;
    if body.get("change")?.as_str()? != "lane_dispatched" || serde_json::from_value::<LaneKind>(Value::from(kind)).is_err() {
        return None;
    }
    let mut current = body.clone();
    current["lane_kind"] = Value::from(kind);
    current["kind"] = Value::from("team_mission");
    Some(current)
}

/// The kinds a `LaneDispatched` was stored under before 2026-11-22.
pub const OLD_LANE_KINDS: [&str; 3] = ["member_agent", "open_web", "peer_agent"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Ev;
    use serde_json::json;

    /// A dispatched lane is written as a mission event and read back as
    /// one; one written in the old form, under its lane's kind, is too.
    #[test]
    fn a_dispatched_lane_is_a_mission_event() {
        let ev = Ev::TeamMission(TeamMissionEvent::LaneDispatched {
            workspace: "w".into(),
            mission: "m".into(),
            lane: "l".into(),
            kind: LaneKind::OpenWeb,
            target_member: None,
            objective: "o".into(),
            scope: String::new(),
            auto_release: true,
            child_task: None,
            child_run: Some("r".into()),
            thread: None,
            routine: None,
        });
        let body = serde_json::to_value(&ev).unwrap();
        assert_eq!((body["kind"].as_str(), body["lane_kind"].as_str()), (Some("team_mission"), Some("open_web")), "the lane's kind took the event's: {body}");
        assert!(matches!(crate::log::parse_event(&body), Ok(Some(Ev::TeamMission(TeamMissionEvent::LaneDispatched { kind: LaneKind::OpenWeb, .. })))));
        let old = json!({ "kind": "member_agent", "change": "lane_dispatched", "workspace": "w", "mission": "m", "lane": "l", "target_member": "bob", "objective": "o", "child_run": "r" });
        let read = crate::log::parse_event(&old).unwrap();
        assert!(matches!(read, Some(Ev::TeamMission(TeamMissionEvent::LaneDispatched { kind: LaneKind::MemberAgent, .. }))), "an old dispatched lane was skipped: {read:?}");
        assert!(current_lane_dispatched(&json!({ "kind": "member_agent", "change": "other" })).is_none());
    }
}
