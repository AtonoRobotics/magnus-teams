//! Async Fan-Out / Fan-In Team Orchestrator & Release Airlock.
//!
//! Replaces the 5-minute synchronous polling loop and 40-message whisper dumps:
//! - Each member lane runs inside that member's isolated Personal Agent boundary
//!   with ONLY a clean, scrubbed objective (never dumping raw room chat).
//! - One-Sentence Rule: a person's own action in a Room authorizes their agent
//!   to share the requested, scrubbed result (`target == requester`, or an
//!   active `StandingRelease`), while cross-user requests hold privately in the
//!   target member's Release Airlock until they release it.
//! - Completion is event-driven: a background watcher checks `scrub_check`,
//!   emits `LaneReleased` or `LaneDraftReady`, and wakes the Leader Agent on
//!   release to update the Living Deliverable.

use crate::compartment::query_gate::scrub_check;
use crate::compartment::{Change as CompChange, Compartment};
use crate::events::{Ev, LaneKind, TeamMissionEvent};
use crate::kernel::{new_id, Kernel, NewRun};
use crate::state::Status;
use crate::sub_agents::SubAgent;
use crate::tools::Label;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Poll interval for the detached lane completion watcher (runs in its own task,
/// never blocking the Leader Agent's tool call).
const LANE_WATCH_POLL_MS: Duration = Duration::from_millis(250);
/// Maximum wall-clock time a detached lane watcher waits before marking the lane failed.
const LANE_WATCH_MAX_SECS: Duration = Duration::from_secs(15 * 60);
/// Maximum length of a single lane objective dispatched by the Leader Agent.
const MAX_OBJECTIVE_CHARS: usize = 2_000;

/// Spec for one parallel lane dispatched by the Team Leader Agent.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LaneSpec {
    pub kind: LaneKind,
    #[serde(default)]
    pub target_member: Option<String>,
    pub objective: String,
    #[serde(default)]
    pub scope: String,
}

impl Kernel {
    /// Starts a new Team Mission and fans out `lanes` in parallel without blocking.
    pub async fn start_mission_with_lanes(
        self: &Arc<Self>,
        workspace: &str,
        room: &str,
        requester: &str,
        goal: &str,
        leader_run: Option<String>,
        lanes: Vec<LaneSpec>,
    ) -> Result<String> {
        let mission_id = new_id("mission");
        let actor = leader_run.as_deref().map(|r| format!("run:{r}")).unwrap_or_else(|| format!("person:{requester}"));
        self.emit(
            &actor,
            Ev::TeamMission(TeamMissionEvent::Started {
                workspace: workspace.to_string(),
                room: room.to_string(),
                mission: mission_id.clone(),
                goal: goal.to_string(),
                requester: requester.to_string(),
                leader_run: leader_run.clone(),
            }),
        )
        .await?;

        for spec in lanes {
            self.dispatch_one_lane(workspace, room, &mission_id, requester, &actor, spec).await?;
        }
        Ok(mission_id)
    }

    /// Dispatches one isolated lane and spawns its detached completion watcher.
    pub async fn dispatch_one_lane(self: &Arc<Self>, workspace: &str, room: &str, mission_id: &str, requester: &str, actor: &str, spec: LaneSpec) -> Result<String> {
        let lane_id = new_id("lane");
        let objective = spec.objective.trim();
        if objective.is_empty() || objective.chars().count() > MAX_OBJECTIVE_CHARS {
            return Err(anyhow::anyhow!("lane objective must be between 1 and {MAX_OBJECTIVE_CHARS} characters"));
        }
        if let Some(reason) = scrub_check(objective) {
            return Err(anyhow::anyhow!("refused lane objective: {reason}"));
        }

        match spec.kind {
            LaneKind::MemberAgent => {
                let target = spec.target_member.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(requester);
                let is_member = self.read(|s| s.team.role(workspace, target).is_some());
                if !is_member || self.person(target).is_none() {
                    return Err(anyhow::anyhow!("{target} is not a member of workspace {workspace}"));
                }
                let auto_release = target == requester || self.read(|s| s.missions.has_standing_release(workspace, room, target, &spec.scope));
                let ws_name = self.read(|s| s.team.workspace(workspace).map(|w| w.name.clone())).unwrap_or_else(|| workspace.to_string());
                let prompt = format!(
                    "Room \"{room}\" in workspace \"{ws_name}\" requested this scoped update from your personal apps (requested by {requester}):\n\n{objective}\n\nReturn a concise, scrubbed summary containing only the requested facts—never raw credentials, private emails, or unrelated personal records."
                );
                // Clean label: untrusted only when requested by someone else; private always.
                let label = Label { untrusted: target != requester, private: true };
                let (child_task, child_run) = self.create_run(actor, target, prompt, vec![], NewRun { label: Some(label), ..Default::default() }).await?;
                self.emit(
                    actor,
                    Ev::TeamMission(TeamMissionEvent::LaneDispatched {
                        workspace: workspace.to_string(),
                        mission: mission_id.to_string(),
                        lane: lane_id.clone(),
                        kind: LaneKind::MemberAgent,
                        target_member: Some(target.to_string()),
                        objective: objective.to_string(),
                        scope: spec.scope.clone(),
                        auto_release,
                        child_task: Some(child_task),
                        child_run: Some(child_run.clone()),
                    }),
                )
                .await?;
                self.wake(&child_run);
                self.spawn_lane_watcher(workspace.to_string(), mission_id.to_string(), lane_id.clone(), Some(target.to_string()), child_run, auto_release);
            }
            LaneKind::OpenWeb => {
                let tools = crate::compartment::open_web_tools(false, self.web.can_search(), self.browser.available());
                let (child_task, child_run) = (new_id("task"), new_id("run"));
                let prompt = format!("Research the following public web question for a team room and return a concise summary with sources:\n\n{objective}");
                self.emit(
                    "kernel",
                    Ev::Compartment(CompChange::Started { run: child_run.clone(), parent: None, compartment: Compartment::OpenWeb { link_following_only: false } }),
                )
                .await?;
                self.emit("kernel", Ev::SubAgent(SubAgent { child: child_run.clone(), parent: actor.to_string(), tools })).await?;
                self.create_run(actor, requester, prompt, vec![], NewRun { label: None, ids: Some((child_task.clone(), child_run.clone())), ..Default::default() }).await?;
                self.emit(
                    actor,
                    Ev::TeamMission(TeamMissionEvent::LaneDispatched {
                        workspace: workspace.to_string(),
                        mission: mission_id.to_string(),
                        lane: lane_id.clone(),
                        kind: LaneKind::OpenWeb,
                        target_member: None,
                        objective: objective.to_string(),
                        scope: spec.scope,
                        auto_release: true,
                        child_task: Some(child_task),
                        child_run: Some(child_run.clone()),
                    }),
                )
                .await?;
                self.wake(&child_run);
                self.spawn_lane_watcher(workspace.to_string(), mission_id.to_string(), lane_id.clone(), None, child_run, true);
            }
            LaneKind::PeerAgent => {
                // PeerAgent lanes execute inside the target member's isolated boundary
                // via the peer_agent surrogate hook (W3.4); record dispatched state.
                self.emit(
                    actor,
                    Ev::TeamMission(TeamMissionEvent::LaneDispatched {
                        workspace: workspace.to_string(),
                        mission: mission_id.to_string(),
                        lane: lane_id.clone(),
                        kind: LaneKind::PeerAgent,
                        target_member: spec.target_member,
                        objective: objective.to_string(),
                        scope: spec.scope,
                        auto_release: false,
                        child_task: None,
                        child_run: None,
                    }),
                )
                .await?;
            }
        }
        Ok(lane_id)
    }

    /// Detached async watcher that routes a finished lane either straight to
    /// `LaneReleased` (when `auto_release` AND `scrub_check` passes) or into
    /// the owner's private Release Airlock (`LaneDraftReady`).
    fn spawn_lane_watcher(self: &Arc<Self>, workspace: String, mission: String, lane: String, owner: Option<String>, child_run: String, auto_release: bool) {
        let k = self.clone();
        tokio::spawn(async move {
            let started = Instant::now();
            loop {
                let Some(r) = k.read(|s| s.runs.get(&child_run).cloned()) else {
                    let _ = k.emit("kernel", Ev::TeamMission(TeamMissionEvent::LaneFailed { workspace, mission, lane, reason: "worker run disappeared".into() })).await;
                    return;
                };
                if r.status.terminal() {
                    if r.status == Status::Failed {
                        let _ = k.emit("kernel", Ev::TeamMission(TeamMissionEvent::LaneFailed { workspace, mission, lane, reason: "worker run failed".into() })).await;
                        return;
                    }
                    let summary = r
                        .summary
                        .clone()
                        .or_else(|| r.replies.last().map(|m| m.text().to_string()))
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| "(finished without a text summary)".to_string());
                    let scrub_flagged = scrub_check(&summary).is_some();
                    if auto_release && !scrub_flagged {
                        let released_by = owner.clone().unwrap_or_else(|| "open_web".to_string());
                        let _ = k
                            .emit(
                                "kernel",
                                Ev::TeamMission(TeamMissionEvent::LaneReleased {
                                    workspace: workspace.clone(),
                                    mission: mission.clone(),
                                    lane,
                                    contribution: summary,
                                    released_by,
                                }),
                            )
                            .await;
                        k.refresh_mission_deliverable(&workspace, &mission).await;
                    } else if let Some(owner_id) = owner {
                        let _ =
                            k.emit("kernel", Ev::TeamMission(TeamMissionEvent::LaneDraftReady { workspace, mission, lane, owner: owner_id, draft: summary, scrub_flagged })).await;
                    }
                    return;
                }
                if started.elapsed() > LANE_WATCH_MAX_SECS {
                    let _ = k.emit("kernel", Ev::TeamMission(TeamMissionEvent::LaneFailed { workspace, mission, lane, reason: "lane timed out".into() })).await;
                    return;
                }
                tokio::time::sleep(LANE_WATCH_POLL_MS).await;
            }
        });
    }

    /// Re-synthesizes the Living Deliverable snapshot from all released lanes
    /// and wakes the Leader Agent run if one is attached.
    pub(crate) async fn refresh_mission_deliverable(self: &Arc<Self>, workspace: &str, mission_id: &str) {
        let Some((markdown, complete, leader_run)) = self.read(|s| {
            let m = s.missions.mission(mission_id)?;
            let mut sections = Vec::new();
            let mut all_done = !m.lanes.is_empty();
            for lane in m.lanes.values() {
                match (&lane.contribution, lane.status) {
                    (Some(text), crate::team::missions::LaneStatus::Released) => {
                        let who = lane.owner.as_deref().unwrap_or("OpenWeb");
                        sections.push(format!("### {} ({})\n{}", lane.objective, who, text.trim()));
                    }
                    _ => {
                        all_done = false;
                    }
                }
            }
            Some((sections.join("\n\n"), all_done, m.leader_run.clone()))
        }) else {
            return;
        };
        let _ = self
            .emit("kernel", Ev::TeamMission(TeamMissionEvent::DeliverableUpdated { workspace: workspace.to_string(), mission: mission_id.to_string(), markdown, complete }))
            .await;
        if let Some(run_id) = leader_run {
            self.wake(&run_id);
        }
    }
}
