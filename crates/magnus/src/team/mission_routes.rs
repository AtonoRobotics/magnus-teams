//! HTTP routes for Team Missions, owner-only Release Airlock actions, Member
//! Capability discovery, and the user avatar.

use super::caller::{at, Member};
use super::missions::LaneStep;
use super::orchestrator::LaneSpec;
use super::routes::{has_channel, K};
use crate::api::who;
use crate::api_error::{refuse, ApiResult};
use crate::compartment::query_gate::scrub_check;
use crate::events::{Ev, ProfileAvatar, TeamMissionEvent};
use crate::kernel::{Kernel, NewRun};
use crate::tools::Label;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

const MAX_RELEASE_CHARS: usize = 16_000;
const MAX_STEER_CHARS: usize = 2_000;
const STEER_POLL_MS: Duration = Duration::from_millis(250);
const MAX_STEER_POLL_TICKS: u32 = 3600;
/// Why a lane's owner who can no longer see its room may not act on it.
const NOT_IN_ROOM: &str = "you are no longer in this lane's conversation";

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/workspaces/{ws}/rooms/{room}/missions", get(list_missions).post(create_mission))
        .route("/v1/workspaces/{ws}/missions/{mission}/lanes/{lane}/release", post(release_lane))
        .route("/v1/workspaces/{ws}/missions/{mission}/lanes/{lane}/steer", post(steer_lane))
        .route("/v1/workspaces/{ws}/capabilities", get(workspace_capabilities))
        .route("/v1/profile/avatar", put(set_avatar))
}

#[derive(Deserialize)]
struct CreateMissionBody {
    goal: String,
    #[serde(default)]
    lanes: Vec<LaneSpec>,
}

#[derive(Deserialize)]
struct ReleaseBody {
    /// Optional edited text; defaults to the lane's private draft.
    #[serde(default)]
    contribution: Option<String>,
    /// When true, saves a standing release rule for this `(workspace, room, scope)`.
    #[serde(default)]
    save_standing_rule: bool,
}

#[derive(Deserialize)]
struct SteerBody {
    instruction: String,
}

async fn list_missions(State(k): K, Member { person: p, .. }: Member<at::Guest>, Path((ws, room)): Path<(String, String)>) -> ApiResult {
    if !has_channel(&k, &ws, &room, &p.id) {
        return refuse(StatusCode::NOT_FOUND, "no such channel");
    }
    let view = k.read(|s| s.missions.view_for_room(&ws, &room, &p.id));
    Ok(Json(view).into_response())
}

async fn create_mission(State(k): K, Member { person: p, .. }: Member<at::Member>, Path((ws, room)): Path<(String, String)>, Json(b): Json<CreateMissionBody>) -> ApiResult {
    if !has_channel(&k, &ws, &room, &p.id) {
        return refuse(StatusCode::NOT_FOUND, "no such channel");
    }
    let goal = b.goal.trim();
    if goal.is_empty() {
        return refuse(StatusCode::BAD_REQUEST, "mission goal is required");
    }
    match k.start_mission_with_lanes(&ws, &room, &p.id, goal, None, b.lanes).await {
        Ok(id) => Ok((StatusCode::CREATED, Json(json!({ "id": id }))).into_response()),
        Err(e) => refuse(StatusCode::BAD_REQUEST, format!("{e:#}")),
    }
}

/// Owner-only Release Airlock action: validates ownership, scrubs credentials,
/// emits `LaneReleased` (plus optional `StandingReleaseSet`), and updates the
/// Living Deliverable.
async fn release_lane(
    State(k): K,
    Member { person: p, .. }: Member<at::Member>,
    Path((ws, mission, lane)): Path<(String, String, String)>,
    Json(b): Json<ReleaseBody>,
) -> ApiResult {
    // A lane still working is the watcher's: the owner releases a draft
    // (`LaneStatus::next`), and `move_lane` checks again as it writes.
    let Some((room, scope, objective, draft)) = k.read(|s| {
        let m = s.missions.mission(&mission).filter(|m| m.workspace == ws)?;
        let l = m.lanes.get(&lane)?;
        if l.owner.as_deref() != Some(p.id.as_str()) || !s.missions.lane_can(&mission, &lane, LaneStep::OwnerRelease) {
            return None;
        }
        Some((m.room.clone(), l.scope.clone(), l.objective.clone(), l.private_draft.clone()))
    }) else {
        return refuse(StatusCode::NOT_FOUND, "no releasable lane owned by you");
    };
    if !has_channel(&k, &ws, &room, &p.id) {
        return refuse(StatusCode::FORBIDDEN, NOT_IN_ROOM);
    }

    let text = b.contribution.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from).or(draft).unwrap_or_default();
    if text.is_empty() || text.chars().count() > MAX_RELEASE_CHARS {
        return refuse(StatusCode::BAD_REQUEST, "released contribution cannot be empty or exceed size limit");
    }
    if let Some(reason) = scrub_check(&text) {
        return refuse(StatusCode::BAD_REQUEST, format!("blocked release: {reason}. Remove credentials before sharing"));
    }

    let actor = format!("person:{}", p.id);
    let released = TeamMissionEvent::LaneReleased { workspace: ws.clone(), mission: mission.clone(), lane: lane.clone(), contribution: text.clone(), released_by: p.id.clone() };
    let moved = k.move_lane(&actor, LaneStep::OwnerRelease, released).await.map_err(|e| Box::new(crate::api::err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))))?;
    if !moved {
        return refuse(StatusCode::CONFLICT, "this lane was released or ended a moment ago");
    }

    if b.save_standing_rule && !scope.trim().is_empty() {
        let scope = super::missions::standing_key(&scope, &objective);
        let _ = k.emit(&actor, Ev::TeamMission(TeamMissionEvent::StandingReleaseSet { workspace: ws.clone(), room, person: p.id.clone(), scope, active: true })).await;
    }

    k.post_released_contribution(&ws, &mission, &p.id, &text, None).await;
    k.refresh_mission_deliverable(&ws, &mission).await;
    Ok(Json(json!({ "released": true })).into_response())
}

/// Owner-only private steer: asks the owner's personal agent to revise their
/// unreleased draft without exposing anything to teammates.
async fn steer_lane(State(k): K, Member { person: p, .. }: Member<at::Member>, Path((ws, mission, lane)): Path<(String, String, String)>, Json(b): Json<SteerBody>) -> ApiResult {
    let instruction = b.instruction.trim();
    if instruction.is_empty() || instruction.chars().count() > MAX_STEER_CHARS {
        return refuse(StatusCode::BAD_REQUEST, "steer instruction must be short and non-empty");
    }
    let Some((room, objective, prev_draft, requester)) = k.read(|s| {
        let m = s.missions.mission(&mission).filter(|m| m.workspace == ws)?;
        let l = m.lanes.get(&lane)?;
        if l.owner.as_deref() != Some(p.id.as_str()) || !s.missions.lane_can(&mission, &lane, LaneStep::Draft) {
            return None;
        }
        Some((m.room.clone(), l.objective.clone(), l.private_draft.clone().unwrap_or_default(), m.requester.clone()))
    }) else {
        return refuse(StatusCode::NOT_FOUND, "no unreleased lane owned by you");
    };
    if !has_channel(&k, &ws, &room, &p.id) {
        return refuse(StatusCode::FORBIDDEN, NOT_IN_ROOM);
    }

    // Told by how it begins, so it goes with the requester's words (`copies`).
    let prompt = super::copies::steer_prompt(&objective, &prev_draft, instruction);
    // The objective is the requester's words and the draft came of them: as
    // untrusted as the lane's own run when someone else asked, and started
    // as that lane was, by the requester, so its words read as another's
    // (`Task::by_other`), never as the owner's own.
    let untrusted = super::owner::from_others(&p.id, [requester.as_str()]);
    let label = Label { untrusted, private: true };
    let by = if untrusted { &requester } else { &p.id };
    let (_, child_run) = k
        .create_run(&format!("person:{by}"), &p.id, prompt, vec![], NewRun { label: Some(label), ..Default::default() })
        .await
        .map_err(|e| Box::new(crate::api::err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))))?;
    k.wake(&child_run);
    let k2 = k.clone();
    let owner = p.id.clone();
    tokio::spawn(async move {
        for _ in 0..MAX_STEER_POLL_TICKS {
            if let Some(r) = k2.read(|s| s.runs.get(&child_run).cloned()) {
                if r.status.terminal() {
                    if let Some(text) = r.summary.or_else(|| r.replies.last().map(|m| m.text().to_string())).filter(|s| !s.trim().is_empty()) {
                        let flagged = scrub_check(&text).is_some();
                        let draft = TeamMissionEvent::LaneDraftReady { workspace: ws, mission, lane, owner, draft: text, scrub_flagged: flagged };
                        let _ = k2.move_lane("kernel", LaneStep::Draft, draft).await;
                    }
                    return;
                }
            }
            tokio::time::sleep(STEER_POLL_MS).await;
        }
    });
    Ok(Json(json!({ "updating": true })).into_response())
}

/// Returns each workspace member's advertised app names and `ProfileAvatar`,
/// never credentials or data. An app its person hid in an older release
/// (`AppVisibilitySet`) stays hidden, though nothing sets one now.
async fn workspace_capabilities(State(k): K, Member { .. }: Member<at::Guest>, Path(ws): Path<String>) -> ApiResult {
    let members: Vec<String> = k.read(|s| s.team.workspace(&ws).map(|w| w.members.keys().cloned().collect()).unwrap_or_default());
    let connectors = k.connectors();
    let out: Vec<Value> = members
        .into_iter()
        .map(|person| {
            let name = k.person(&person).map(|p| p.name).unwrap_or_else(|| person.clone());
            let avatar = k.read(|s| s.missions.avatar_of(&person));
            let apps: Vec<Value> = connectors
                .iter()
                .filter(|c| k.connected(c, &person) && !k.lapsed(c, &person) && !k.read(|s| s.missions.is_app_hidden(&person, &c.name)))
                .map(|c| json!({ "name": c.name, "title": c.title.clone().unwrap_or_else(|| crate::connectors::title(&c.name)) }))
                .collect();
            json!({ "id": person, "name": name, "avatar": avatar, "apps": apps })
        })
        .collect();
    Ok(Json(out).into_response())
}

async fn set_avatar(State(k): K, h: HeaderMap, Json(avatar): Json<ProfileAvatar>) -> ApiResult {
    let p = who(&k, &h).await?;
    let actor = format!("person:{}", p.id);
    k.emit(&actor, Ev::TeamMission(TeamMissionEvent::AvatarSet { person: p.id.clone(), avatar: avatar.clone() }))
        .await
        .map_err(|e| Box::new(crate::api::err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))))?;
    Ok(Json(json!({ "avatar": avatar })).into_response())
}
