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
pub(crate) const MAX_OBJECTIVE_CHARS: usize = 2_000;
/// Most lanes one mission fans out to, and the longest goal and scope label.
const MAX_MISSION_LANES: usize = 8;
const MAX_GOAL_CHARS: usize = 2_000;
const MAX_SCOPE_CHARS: usize = 200;

/// Spec for one parallel lane dispatched by the Team Leader Agent or a member request.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LaneSpec {
    pub kind: LaneKind,
    #[serde(default)]
    pub target_member: Option<String>,
    pub objective: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub model: Option<(String, String)>,
    #[serde(default)]
    pub thread: Option<String>,
    /// Hold the answer in the owner's Airlock even when the owner asked:
    /// set when the agent was only named, not called on.
    #[serde(default)]
    pub hold: bool,
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
        if goal.chars().count() > MAX_GOAL_CHARS || lanes.len() > MAX_MISSION_LANES {
            return Err(anyhow::anyhow!("a mission's goal is at most {MAX_GOAL_CHARS} characters and it has at most {MAX_MISSION_LANES} lanes"));
        }
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
            self.dispatch_lane_inner(workspace, room, &mission_id, requester, &actor, spec).await?;
        }
        Ok(mission_id)
    }

    /// Starts a personal-agent Mission for a single member's `@Magnus` request
    /// in a shared conversation. The requester's own personal Magnus runs in
    /// isolation and posts its scrubbed answer back to the room (or holds it
    /// in their Release Airlock if `scrub_check` flags anything).
    pub async fn start_personal_room_mission(
        self: &Arc<Self>,
        workspace: &str,
        room: &str,
        requester: &str,
        objective: &str,
        thread: Option<String>,
        model: Option<(String, String)>,
        hold: bool,
    ) -> Result<(String, String, String)> {
        let mission_id = new_id("mission");
        let actor = format!("person:{requester}");
        self.emit(
            &actor,
            Ev::TeamMission(TeamMissionEvent::Started {
                workspace: workspace.to_string(),
                room: room.to_string(),
                mission: mission_id.clone(),
                goal: objective.to_string(),
                requester: requester.to_string(),
                leader_run: None,
            }),
        )
        .await?;
        let spec =
            LaneSpec { kind: LaneKind::MemberAgent, target_member: Some(requester.to_string()), objective: objective.to_string(), scope: String::new(), model, thread, hold };
        let (_, task, run) = self.dispatch_lane_inner(workspace, room, &mission_id, requester, &actor, spec).await?;
        Ok((mission_id, task.unwrap_or_default(), run.unwrap_or_default()))
    }

    /// Dispatches one isolated lane and spawns its detached completion watcher.
    pub async fn dispatch_one_lane(self: &Arc<Self>, workspace: &str, room: &str, mission_id: &str, requester: &str, actor: &str, spec: LaneSpec) -> Result<String> {
        let (lane_id, _, _) = self.dispatch_lane_inner(workspace, room, mission_id, requester, actor, spec).await?;
        Ok(lane_id)
    }

    async fn dispatch_lane_inner(
        self: &Arc<Self>,
        workspace: &str,
        room: &str,
        mission_id: &str,
        requester: &str,
        actor: &str,
        spec: LaneSpec,
    ) -> Result<(String, Option<String>, Option<String>)> {
        let lane_id = new_id("lane");
        let objective = spec.objective.trim();
        if objective.is_empty() || objective.chars().count() > MAX_OBJECTIVE_CHARS {
            return Err(anyhow::anyhow!("lane objective must be between 1 and {MAX_OBJECTIVE_CHARS} characters"));
        }
        if let Some(reason) = scrub_check(objective) {
            return Err(anyhow::anyhow!("refused lane objective: {reason}"));
        }
        if spec.scope.chars().count() > MAX_SCOPE_CHARS {
            return Err(anyhow::anyhow!("a lane's scope is at most {MAX_SCOPE_CHARS} characters"));
        }

        match spec.kind {
            LaneKind::MemberAgent => {
                let target = spec.target_member.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(requester);
                // Someone else's agent works only for a member who is in the
                // room, never a guest, on the model their own settings pick.
                let fits = self.read(|s| {
                    let w = s.team.workspace(workspace);
                    let role = s.team.role(workspace, target);
                    role.is_some() && w.is_some_and(|w| w.sees(room, target)) && (target == requester || role != Some(crate::team::Role::Guest))
                });
                if !fits || self.person(target).is_none() {
                    return Err(anyhow::anyhow!("{target} is not a member in this room of workspace {workspace}"));
                }
                let model = if target == requester { spec.model } else { None };
                let auto_release = !spec.hold && (target == requester || self.read(|s| s.missions.has_standing_release(workspace, room, target, &spec.scope, objective)));
                let ws_name = self.read(|s| s.team.workspace(workspace).map(|w| w.name.clone())).unwrap_or_else(|| workspace.to_string());
                let prompt = format!(
                    "Room \"{room}\" in workspace \"{ws_name}\" requested this scoped update from your personal apps (requested by {requester}):\n\n{objective}\n\nReturn a concise, scrubbed summary containing only the requested facts—never raw credentials, private emails, or unrelated personal records."
                );
                // Clean label: untrusted only when requested by someone else; private always.
                let label = Label { untrusted: target != requester, private: true };
                let (child_task, child_run) = self.create_run(actor, target, prompt, vec![], NewRun { label: Some(label), model, ..Default::default() }).await?;
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
                        child_task: Some(child_task.clone()),
                        child_run: Some(child_run.clone()),
                    }),
                )
                .await?;
                self.wake(&child_run);
                self.spawn_lane_watcher(workspace.to_string(), mission_id.to_string(), lane_id.clone(), Some(target.to_string()), child_run.clone(), auto_release, spec.thread);
                Ok((lane_id, Some(child_task), Some(child_run)))
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
                self.create_run(
                    actor,
                    requester,
                    prompt,
                    vec![],
                    NewRun { label: None, ids: Some((child_task.clone(), child_run.clone())), model: spec.model, ..Default::default() },
                )
                .await?;
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
                        child_task: Some(child_task.clone()),
                        child_run: Some(child_run.clone()),
                    }),
                )
                .await?;
                self.wake(&child_run);
                self.spawn_lane_watcher(workspace.to_string(), mission_id.to_string(), lane_id.clone(), None, child_run.clone(), true, spec.thread);
                Ok((lane_id, Some(child_task), Some(child_run)))
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
                Ok((lane_id, None, None))
            }
        }
    }

    /// Detached async watcher that routes a finished lane either straight to
    /// `LaneReleased` (when `auto_release` AND `scrub_check` passes) or into
    /// the owner's private Release Airlock (`LaneDraftReady`).
    #[allow(clippy::too_many_arguments)]
    fn spawn_lane_watcher(
        self: &Arc<Self>,
        workspace: String,
        mission: String,
        lane: String,
        owner: Option<String>,
        child_run: String,
        auto_release: bool,
        thread: Option<String>,
    ) {
        let k = self.clone();
        tokio::spawn(async move {
            let started = Instant::now();
            loop {
                let Some(r) = k.read(|s| s.runs.get(&child_run).cloned()) else {
                    let _ = k.emit("kernel", Ev::TeamMission(TeamMissionEvent::LaneFailed { workspace, mission, lane, reason: "worker run disappeared".into() })).await;
                    return;
                };
                if r.status.terminal() {
                    // Only a finished run is anything to share: a stopped
                    // one (its person's Stop, their removal) shares nothing.
                    if r.status != Status::Completed {
                        let reason = if r.status == Status::Cancelled { "worker run was stopped" } else { "worker run failed" };
                        let _ = k.emit("kernel", Ev::TeamMission(TeamMissionEvent::LaneFailed { workspace, mission, lane, reason: reason.into() })).await;
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
                                    contribution: summary.clone(),
                                    released_by: released_by.clone(),
                                }),
                            )
                            .await;
                        k.post_released_contribution(&workspace, &mission, &released_by, &summary, thread).await;
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

    /// Posts a released lane's scrubbed contribution into its room's chat
    /// stream so collaborators see the shared work inline as well as in the
    /// Missions canvas. Attributed to `agent:<released_by>` so erasure and
    /// message permissions stay tied to the contributor.
    pub(crate) async fn post_released_contribution(self: &Arc<Self>, workspace: &str, mission_id: &str, released_by: &str, contribution: &str, thread: Option<String>) {
        let Some(room) = self.read(|s| {
            let m = s.missions.mission(mission_id)?;
            let w = s.team.workspace(workspace)?;
            w.channels.contains_key(&m.room).then(|| m.room.clone())
        }) else {
            return;
        };
        let sender = format!("agent:{released_by}");
        let message = super::Message { id: new_id("msg"), sender, text: contribution.to_string(), thread, at: chrono::Utc::now() };
        let actor = format!("person:{released_by}");
        let _ = self.emit(&actor, Ev::Team(super::Change::Message { workspace: workspace.to_string(), channel: room, message })).await;
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
