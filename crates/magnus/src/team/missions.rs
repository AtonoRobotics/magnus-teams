//! Team Missions ledger: async Fan-Out/Fan-In collaborator lanes, private-to-owner
//! Release Airlock drafts, standing release rules, app visibility, and user avatars.
//!
//! Enforces strict per-user isolation:
//! - A lane's `private_draft` is NEVER returned to any caller other than `lane.owner`.
//! - A `TeamContribution` becomes visible to the Room and Leader Agent only after
//!   explicit release (`LaneReleased`).

use crate::events::{LaneKind, ProfileAvatar, TeamMissionEvent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LaneStatus {
    Working,
    WaitingRelease,
    Released,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Lane {
    pub id: String,
    pub kind: LaneKind,
    pub owner: Option<String>,
    pub objective: String,
    pub scope: String,
    pub auto_release: bool,
    pub child_task: Option<String>,
    pub child_run: Option<String>,
    pub status: LaneStatus,
    /// Strictly private to `owner` until released across the Airlock.
    pub private_draft: Option<String>,
    pub scrub_flagged: bool,
    /// Scrubbed contribution visible to the Room & Leader once released.
    pub contribution: Option<String>,
    pub released_by: Option<String>,
    pub failure_reason: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Mission {
    pub id: String,
    pub workspace: String,
    pub room: String,
    pub goal: String,
    pub requester: String,
    pub leader_run: Option<String>,
    pub lanes: BTreeMap<String, Lane>,
    pub deliverable: String,
    pub complete: bool,
}

#[derive(Clone, Debug, Default)]
pub struct MissionLedger {
    missions: BTreeMap<String, Mission>,
    /// `(workspace, room, person, scope)` keys where `person` authorized standing auto-release.
    standing_releases: BTreeSet<(String, String, String, String)>,
    /// Per-person display avatar (`Default | Preset | Photo`).
    avatars: BTreeMap<String, ProfileAvatar>,
    /// Personal apps a user chose to hide from Team capability discovery.
    hidden_apps: BTreeMap<String, BTreeSet<String>>,
}

impl crate::forget::Forget for MissionLedger {
    fn forget(&mut self, who: &crate::forget::Erased) {
        let p = who.person.as_str();
        self.avatars.retain(|person, _| person != p);
        self.hidden_apps.retain(|person, _| person != p);
        self.standing_releases.retain(|(_, _, person, _)| person != p);
        for m in self.missions.values_mut() {
            for lane in m.lanes.values_mut() {
                if lane.owner.as_deref() == Some(p) {
                    lane.private_draft = None;
                }
            }
        }
    }
}

impl MissionLedger {
    pub fn apply(&mut self, ev: &TeamMissionEvent) {
        match ev {
            TeamMissionEvent::Started { workspace, room, mission, goal, requester, leader_run } => {
                self.missions.insert(
                    mission.clone(),
                    Mission {
                        id: mission.clone(),
                        workspace: workspace.clone(),
                        room: room.clone(),
                        goal: goal.clone(),
                        requester: requester.clone(),
                        leader_run: leader_run.clone(),
                        lanes: BTreeMap::new(),
                        deliverable: String::new(),
                        complete: false,
                    },
                );
            }
            TeamMissionEvent::LaneDispatched { mission, lane, kind, target_member, objective, scope, auto_release, child_task, child_run, .. } => {
                if let Some(m) = self.missions.get_mut(mission) {
                    m.lanes.insert(
                        lane.clone(),
                        Lane {
                            id: lane.clone(),
                            kind: *kind,
                            owner: target_member.clone(),
                            objective: objective.clone(),
                            scope: scope.clone(),
                            auto_release: *auto_release,
                            child_task: child_task.clone(),
                            child_run: child_run.clone(),
                            status: LaneStatus::Working,
                            private_draft: None,
                            scrub_flagged: false,
                            contribution: None,
                            released_by: None,
                            failure_reason: None,
                        },
                    );
                }
            }
            TeamMissionEvent::LaneDraftReady { mission, lane, owner, draft, scrub_flagged, .. } => {
                if let Some(l) = self.missions.get_mut(mission).and_then(|m| m.lanes.get_mut(lane)) {
                    if l.owner.as_deref() == Some(owner.as_str()) && l.status != LaneStatus::Released {
                        l.private_draft = Some(draft.clone());
                        l.scrub_flagged = *scrub_flagged;
                        l.status = LaneStatus::WaitingRelease;
                    }
                }
            }
            TeamMissionEvent::LaneReleased { mission, lane, contribution, released_by, .. } => {
                if let Some(l) = self.missions.get_mut(mission).and_then(|m| m.lanes.get_mut(lane)) {
                    l.contribution = Some(contribution.clone());
                    l.released_by = Some(released_by.clone());
                    l.private_draft = None;
                    l.scrub_flagged = false;
                    l.status = LaneStatus::Released;
                }
            }
            TeamMissionEvent::LaneFailed { mission, lane, reason, .. } => {
                if let Some(l) = self.missions.get_mut(mission).and_then(|m| m.lanes.get_mut(lane)) {
                    l.failure_reason = Some(reason.clone());
                    l.status = LaneStatus::Failed;
                }
            }
            TeamMissionEvent::DeliverableUpdated { mission, markdown, complete, .. } => {
                if let Some(m) = self.missions.get_mut(mission) {
                    m.deliverable = markdown.clone();
                    m.complete = *complete;
                }
            }
            TeamMissionEvent::StandingReleaseSet { workspace, room, person, scope, active } => {
                let key = (workspace.clone(), room.clone(), person.clone(), scope.to_ascii_lowercase());
                if *active {
                    self.standing_releases.insert(key);
                } else {
                    self.standing_releases.remove(&key);
                }
            }
            TeamMissionEvent::AvatarSet { person, avatar } => {
                self.avatars.insert(person.clone(), avatar.clone());
            }
            TeamMissionEvent::AppVisibilitySet { person, app, hidden } => {
                let set = self.hidden_apps.entry(person.clone()).or_default();
                if *hidden {
                    set.insert(app.clone());
                } else {
                    set.remove(app);
                }
            }
        }
    }

    pub fn mission(&self, id: &str) -> Option<&Mission> {
        self.missions.get(id)
    }

    pub fn has_standing_release(&self, workspace: &str, room: &str, person: &str, scope: &str) -> bool {
        if scope.trim().is_empty() {
            return false;
        }
        self.standing_releases.contains(&(workspace.to_string(), room.to_string(), person.to_string(), scope.to_ascii_lowercase()))
    }

    pub fn avatar_of(&self, person: &str) -> ProfileAvatar {
        self.avatars.get(person).cloned().unwrap_or_default()
    }

    pub fn is_app_hidden(&self, person: &str, app: &str) -> bool {
        self.hidden_apps.get(person).is_some_and(|s| s.contains(app))
    }

    /// Returns all missions in `(workspace, room)` formatted for `viewer`.
    /// Strictly enforces Airlock privacy: `private_draft` is included ONLY
    /// when `lane.owner == Some(viewer)`.
    pub fn view_for_room(&self, workspace: &str, room: &str, viewer: &str) -> Vec<Value> {
        self.missions
            .values()
            .filter(|m| m.workspace == workspace && m.room == room)
            .map(|m| {
                let lanes: Vec<Value> = m
                    .lanes
                    .values()
                    .map(|l| {
                        let is_owner = l.owner.as_deref() == Some(viewer);
                        json!({
                            "id": l.id,
                            "kind": l.kind,
                            "owner": l.owner,
                            "objective": l.objective,
                            "scope": l.scope,
                            "status": l.status,
                            "auto_release": l.auto_release,
                            "contribution": l.contribution,
                            "released_by": l.released_by,
                            "failure_reason": l.failure_reason,
                            "private_draft": if is_owner { l.private_draft.as_deref() } else { None },
                            "scrub_flagged": if is_owner { l.scrub_flagged } else { false },
                            "can_release": is_owner && l.status == LaneStatus::WaitingRelease,
                        })
                    })
                    .collect();
                let released = m.lanes.values().filter(|l| l.status == LaneStatus::Released).count();
                json!({
                    "id": m.id,
                    "workspace": m.workspace,
                    "room": m.room,
                    "goal": m.goal,
                    "requester": m.requester,
                    "deliverable": m.deliverable,
                    "complete": m.complete,
                    "released_count": released,
                    "total_lanes": m.lanes.len(),
                    "lanes": lanes,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_draft_is_visible_only_to_lane_owner_until_released() {
        let mut ledger = MissionLedger::default();
        ledger.apply(&TeamMissionEvent::Started {
            workspace: "ws1".into(),
            room: "q3".into(),
            mission: "m1".into(),
            goal: "Board update".into(),
            requester: "alice".into(),
            leader_run: Some("r_leader".into()),
        });
        ledger.apply(&TeamMissionEvent::LaneDispatched {
            workspace: "ws1".into(),
            mission: "m1".into(),
            lane: "l_bob".into(),
            kind: LaneKind::MemberAgent,
            target_member: Some("bob".into()),
            objective: "Check GitHub blockers".into(),
            scope: "weekly_blockers".into(),
            auto_release: false,
            child_task: Some("t_bob".into()),
            child_run: Some("r_bob".into()),
        });
        ledger.apply(&TeamMissionEvent::LaneDraftReady {
            workspace: "ws1".into(),
            mission: "m1".into(),
            lane: "l_bob".into(),
            owner: "bob".into(),
            draft: "Private draft: #422 crash".into(),
            scrub_flagged: false,
        });

        // Alice (peer) MUST NOT see Bob's unreleased private_draft.
        let alice_view = ledger.view_for_room("ws1", "q3", "alice");
        assert_eq!(alice_view[0]["lanes"][0]["private_draft"], Value::Null);
        assert_eq!(alice_view[0]["lanes"][0]["can_release"], false);

        // Bob (owner) sees his own private_draft and can release it.
        let bob_view = ledger.view_for_room("ws1", "q3", "bob");
        assert_eq!(bob_view[0]["lanes"][0]["private_draft"], "Private draft: #422 crash");
        assert_eq!(bob_view[0]["lanes"][0]["can_release"], true);

        // Once released, contribution is visible to Alice and private_draft is cleared.
        ledger.apply(&TeamMissionEvent::LaneReleased {
            workspace: "ws1".into(),
            mission: "m1".into(),
            lane: "l_bob".into(),
            contribution: "1 blocker: #422 crash".into(),
            released_by: "bob".into(),
        });
        let alice_after = ledger.view_for_room("ws1", "q3", "alice");
        assert_eq!(alice_after[0]["lanes"][0]["contribution"], "1 blocker: #422 crash");
        assert_eq!(alice_after[0]["lanes"][0]["private_draft"], Value::Null);
        assert_eq!(alice_after[0]["released_count"], 1);
    }

    #[test]
    fn standing_release_and_profile_preferences_roundtrip() {
        let mut ledger = MissionLedger::default();
        assert!(!ledger.has_standing_release("ws1", "q3", "bob", "weekly_blockers"));
        ledger.apply(&TeamMissionEvent::StandingReleaseSet { workspace: "ws1".into(), room: "q3".into(), person: "bob".into(), scope: "Weekly_Blockers".into(), active: true });
        assert!(ledger.has_standing_release("ws1", "q3", "bob", "weekly_blockers"));

        ledger.apply(&TeamMissionEvent::AvatarSet { person: "bob".into(), avatar: ProfileAvatar::Preset { id: "aurora-emerald".into() } });
        assert_eq!(ledger.avatar_of("bob"), ProfileAvatar::Preset { id: "aurora-emerald".into() });
        assert_eq!(ledger.avatar_of("alice"), ProfileAvatar::Default);

        ledger.apply(&TeamMissionEvent::AppVisibilitySet { person: "bob".into(), app: "google-gmail".into(), hidden: true });
        assert!(ledger.is_app_hidden("bob", "google-gmail"));
        assert!(!ledger.is_app_hidden("bob", "github"));
    }
}
