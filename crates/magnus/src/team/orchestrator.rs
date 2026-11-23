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
use crate::sub_agents::SubAgent;
use crate::tools::Label;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Maximum length of a single lane objective dispatched by the Leader Agent.
pub(crate) const MAX_OBJECTIVE_CHARS: usize = 2_000;
/// Most lanes one mission fans out to, and the longest goal and scope label.
const MAX_MISSION_LANES: usize = 8;
const MAX_GOAL_CHARS: usize = 2_000;
const MAX_SCOPE_CHARS: usize = 200;
/// Why a `peer_agent` lane starts nothing: the kind stays only so lanes a
/// log already holds still read (2026-11-22-retire-team-agent-runs).
const PEER_AGENT_RETIRED: &str = "a peer_agent lane is retired; ask for a member_agent lane";

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
    /// The reasoning effort the requester picked for their own agent. Set
    /// only from a checked request (`say::Pick`), never from a lane's body.
    #[serde(skip)]
    pub reasoning: Option<String>,
}

/// Whose agent a member lane runs: the member named, else the requester's own.
fn lane_target<'a>(spec: &'a LaneSpec, requester: &'a str) -> &'a str {
    spec.target_member.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(requester)
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
        // Every lane checked and admitted before anything is recorded, so no
        // mission is left half made and no lane runs for one refused.
        for spec in &lanes {
            self.check_lane(workspace, room, requester, spec).await?;
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
    #[allow(clippy::too_many_arguments)]
    pub async fn start_personal_room_mission(
        self: &Arc<Self>,
        workspace: &str,
        room: &str,
        requester: &str,
        objective: &str,
        thread: Option<String>,
        pick: crate::say::Pick,
        hold: bool,
    ) -> Result<(String, String, String)> {
        self.start_room_mission_picked(&format!("person:{requester}"), workspace, room, requester, objective, thread, pick, hold).await
    }

    /// The same mission for a member, started by `actor`: the member, or
    /// "poll" for work the server starts on their behalf (a routine of
    /// theirs, a follow-up on their question), which runs in the background
    /// and asks no one.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn start_room_mission_as(
        self: &Arc<Self>,
        actor: &str,
        workspace: &str,
        room: &str,
        requester: &str,
        objective: &str,
        thread: Option<String>,
        model: Option<(String, String)>,
        hold: bool,
    ) -> Result<(String, String, String)> {
        self.start_room_mission_picked(actor, workspace, room, requester, objective, thread, crate::say::Pick { model, reasoning: None }, hold).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn start_room_mission_picked(
        self: &Arc<Self>,
        actor: &str,
        workspace: &str,
        room: &str,
        requester: &str,
        objective: &str,
        thread: Option<String>,
        pick: crate::say::Pick,
        hold: bool,
    ) -> Result<(String, String, String)> {
        let spec = LaneSpec {
            kind: LaneKind::MemberAgent,
            target_member: Some(requester.to_string()),
            objective: objective.to_string(),
            scope: String::new(),
            model: pick.model,
            thread,
            hold,
            reasoning: pick.reasoning,
        };
        self.check_lane(workspace, room, requester, &spec).await?;
        let mission_id = new_id("mission");
        let actor = actor.to_string();
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
        let (_, task, run) = self.dispatch_lane_inner(workspace, room, &mission_id, requester, &actor, spec).await?;
        Ok((mission_id, task.unwrap_or_default(), run.unwrap_or_default()))
    }

    /// Every check a lane must pass before its mission is recorded: its
    /// objective and scope, who it works for, and that their run would be
    /// admitted now, by the same checks `create_run` makes as it starts it.
    async fn check_lane(&self, workspace: &str, room: &str, requester: &str, spec: &LaneSpec) -> Result<()> {
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
        let (person, model) = match spec.kind {
            LaneKind::MemberAgent => {
                let target = lane_target(spec, requester);
                // Someone else's agent works only for a member who is in the
                // room, never a guest, on the model their own settings pick.
                let role = self.read(|s| s.team.role(workspace, target));
                let fits = super::routes::has_channel(self, workspace, room, target) && (target == requester || role != Some(crate::team::Role::Guest));
                if !fits || self.person(target).is_none() {
                    return Err(anyhow::anyhow!("{target} is not a member in this room of workspace {workspace}"));
                }
                (target, if target == requester { spec.model.clone() } else { None })
            }
            LaneKind::OpenWeb => (requester, spec.model.clone()),
            LaneKind::PeerAgent => return Err(anyhow::anyhow!("{PEER_AGENT_RETIRED}")),
        };
        if let Some(e) = self.refuse_new(person, false) {
            return Err(e);
        }
        self.admit_run(person, model, self.cfg.budget.clone()).await.map(|_| ())
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
        self.check_lane(workspace, room, requester, &spec).await?;
        let lane_id = new_id("lane");
        let objective = spec.objective.trim();
        match spec.kind {
            LaneKind::MemberAgent => {
                let target = lane_target(&spec, requester).to_string();
                let target = target.as_str();
                let (model, reasoning) = if target == requester { (spec.model.clone(), spec.reasoning.clone()) } else { (None, None) };
                let auto_release = !spec.hold && (target == requester || self.read(|s| s.missions.has_standing_release(workspace, room, target, &spec.scope, objective)));
                let ws_name = self.read(|s| s.team.workspace(workspace).map(|w| w.name.clone())).unwrap_or_else(|| workspace.to_string());
                let prompt = format!(
                    "Room \"{room}\" in workspace \"{ws_name}\" requested this scoped update from your personal apps (requested by {requester}):\n\n{objective}\n\nReturn a concise, scrubbed summary containing only the requested facts—never raw credentials, private emails, or unrelated personal records."
                );
                // Clean label: untrusted only when requested by someone else; private always.
                let label = Label { untrusted: super::owner::from_others(target, [requester]), private: true };
                let (child_task, child_run) = self.create_run(actor, target, prompt, vec![], NewRun { label: Some(label), model, reasoning, ..Default::default() }).await?;
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
                        thread: spec.thread.clone(),
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
                        thread: spec.thread.clone(),
                    }),
                )
                .await?;
                self.wake(&child_run);
                self.spawn_lane_watcher(workspace.to_string(), mission_id.to_string(), lane_id.clone(), None, child_run.clone(), true, spec.thread);
                Ok((lane_id, Some(child_task), Some(child_run)))
            }
            LaneKind::PeerAgent => Err(anyhow::anyhow!("{PEER_AGENT_RETIRED}")),
        }
    }

    /// Posts a released lane's scrubbed contribution into its room's chat
    /// stream so collaborators see the shared work inline as well as in the
    /// Missions canvas. Attributed to `agent:<released_by>` so erasure and
    /// message permissions stay tied to the contributor.
    pub(crate) async fn post_released_contribution(self: &Arc<Self>, workspace: &str, mission_id: &str, released_by: &str, contribution: &str, thread: Option<String>) {
        let Some(room) = self.read(|s| s.missions.mission(mission_id).map(|m| m.room.clone())) else { return };
        // Posted only while they may still see the room, as any post of theirs.
        if !super::routes::has_channel(self, workspace, &room, released_by) {
            return;
        }
        let sender = format!("agent:{released_by}");
        let message = super::Message { id: new_id("msg"), sender, text: contribution.to_string(), thread, at: chrono::Utc::now() };
        let actor = format!("person:{released_by}");
        let _ = self.emit(&actor, Ev::Team(super::Change::Message { workspace: workspace.to_string(), channel: room, message })).await;
    }

    /// Records whether every lane is in, and wakes the Leader Agent run if
    /// one is attached. The deliverable itself is read from the lanes
    /// (`Mission::deliverable`), so the event keeps no copy of their words.
    pub(crate) async fn refresh_mission_deliverable(self: &Arc<Self>, workspace: &str, mission_id: &str) {
        let Some((complete, leader_run)) = self.read(|s| s.missions.mission(mission_id).map(|m| (m.all_released(), m.leader_run.clone()))) else {
            return;
        };
        let markdown = String::new();
        let _ = self
            .emit("kernel", Ev::TeamMission(TeamMissionEvent::DeliverableUpdated { workspace: workspace.to_string(), mission: mission_id.to_string(), markdown, complete }))
            .await;
        if let Some(run_id) = leader_run {
            self.wake(&run_id);
        }
    }
}
