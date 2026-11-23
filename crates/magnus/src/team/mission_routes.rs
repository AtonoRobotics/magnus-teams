//! HTTP routes for Team Missions, owner-only Release Airlock actions, Member
//! Capability discovery, and user avatar / team-app visibility preferences.

use super::caller::{at, Member};
use super::missions::LaneStatus;
use super::orchestrator::LaneSpec;
use super::routes::K;
use crate::api::who;
use crate::api_error::{refuse, ApiResult};
use crate::compartment::query_gate::scrub_check;
use crate::events::{Ev, ProfileAvatar, TeamMissionEvent};
use crate::kernel::{Kernel, NewRun};
use crate::tools::Label;
use axum::extract::{Path, Query, State};
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
const DEFAULT_MEMORY_LIMIT: usize = 20;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/workspaces/{ws}/rooms/{room}/missions", get(list_missions).post(create_mission))
        .route("/v1/workspaces/{ws}/missions/{mission}/lanes/{lane}/release", post(release_lane))
        .route("/v1/workspaces/{ws}/missions/{mission}/lanes/{lane}/steer", post(steer_lane))
        .route("/v1/workspaces/{ws}/capabilities", get(workspace_capabilities))
        .route("/v1/workspaces/{ws}/memory", get(workspace_memory))
        .route("/v1/profile/avatar", put(set_avatar))
        .route("/v1/profile/team-apps", put(set_app_visibility))
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

#[derive(Deserialize)]
struct AppVisibilityBody {
    app: String,
    hidden: bool,
}

async fn list_missions(State(k): K, Member { person: p, .. }: Member<at::Guest>, Path((ws, room)): Path<(String, String)>) -> ApiResult {
    let view = k.read(|s| s.missions.view_for_room(&ws, &room, &p.id));
    Ok(Json(view).into_response())
}

async fn create_mission(State(k): K, Member { person: p, .. }: Member<at::Member>, Path((ws, room)): Path<(String, String)>, Json(b): Json<CreateMissionBody>) -> ApiResult {
    let goal = b.goal.trim();
    if goal.is_empty() {
        return refuse(StatusCode::BAD_REQUEST, "mission goal is required");
    }
    match k.start_mission_with_lanes(&ws, &room, &p.id, goal, None, b.lanes).await {
        Ok(id) => Ok((StatusCode::CREATED, Json(json!({ "id": id }))).into_response()),
        Err(e) => refuse(StatusCode::BAD_REQUEST, &format!("{e:#}")),
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
    let Some((room, scope, draft)) = k.read(|s| {
        let m = s.missions.mission(&mission).filter(|m| m.workspace == ws)?;
        let l = m.lanes.get(&lane)?;
        if l.owner.as_deref() != Some(p.id.as_str()) || l.status == LaneStatus::Released {
            return None;
        }
        Some((m.room.clone(), l.scope.clone(), l.private_draft.clone()))
    }) else {
        return refuse(StatusCode::NOT_FOUND, "no releasable lane owned by you");
    };

    let text = b.contribution.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from).or(draft).unwrap_or_default();
    if text.is_empty() || text.chars().count() > MAX_RELEASE_CHARS {
        return refuse(StatusCode::BAD_REQUEST, "released contribution cannot be empty or exceed size limit");
    }
    if let Some(reason) = scrub_check(&text) {
        return refuse(StatusCode::BAD_REQUEST, &format!("blocked release: {reason}. Remove credentials before sharing"));
    }

    let actor = format!("person:{}", p.id);
    k.emit(
        &actor,
        Ev::TeamMission(TeamMissionEvent::LaneReleased {
            workspace: ws.clone(),
            mission: mission.clone(),
            lane: lane.clone(),
            contribution: text.clone(),
            released_by: p.id.clone(),
        }),
    )
    .await
    .map_err(|e| Box::new(crate::api::err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))))?;

    if b.save_standing_rule && !scope.trim().is_empty() {
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
    let Some((objective, prev_draft)) = k.read(|s| {
        let m = s.missions.mission(&mission).filter(|m| m.workspace == ws)?;
        let l = m.lanes.get(&lane)?;
        if l.owner.as_deref() != Some(p.id.as_str()) || l.status == LaneStatus::Released {
            return None;
        }
        Some((l.objective.clone(), l.private_draft.clone().unwrap_or_default()))
    }) else {
        return refuse(StatusCode::NOT_FOUND, "no unreleased lane owned by you");
    };

    let actor = format!("person:{}", p.id);
    let prompt = format!("Revise your private draft for \"{objective}\" using the owner's instruction.\n\nCurrent draft:\n{prev_draft}\n\nOwner's edit:\n{instruction}");
    let label = Label { untrusted: false, private: true };
    let (_, child_run) = k
        .create_run(&actor, &p.id, prompt, vec![], NewRun { label: Some(label), ..Default::default() })
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
                        let _ =
                            k2.emit("kernel", Ev::TeamMission(TeamMissionEvent::LaneDraftReady { workspace: ws, mission, lane, owner, draft: text, scrub_flagged: flagged })).await;
                    }
                    return;
                }
            }
            tokio::time::sleep(STEER_POLL_MS).await;
        }
    });
    Ok(Json(json!({ "updating": true })).into_response())
}

/// Returns each workspace member's advertised app names (respecting their
/// `hidden_apps` toggle) and `ProfileAvatar` — never credentials or data.
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
    let p = who(&k, &h).await.map_err(Box::new)?;
    let actor = format!("person:{}", p.id);
    k.emit(&actor, Ev::TeamMission(TeamMissionEvent::AvatarSet { person: p.id.clone(), avatar: avatar.clone() }))
        .await
        .map_err(|e| Box::new(crate::api::err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))))?;
    Ok(Json(json!({ "avatar": avatar })).into_response())
}

async fn set_app_visibility(State(k): K, h: HeaderMap, Json(b): Json<AppVisibilityBody>) -> ApiResult {
    let p = who(&k, &h).await.map_err(Box::new)?;
    let app = b.app.trim();
    if app.is_empty() {
        return refuse(StatusCode::BAD_REQUEST, "app name is required");
    }
    let actor = format!("person:{}", p.id);
    k.emit(&actor, Ev::TeamMission(TeamMissionEvent::AppVisibilitySet { person: p.id.clone(), app: app.to_string(), hidden: b.hidden }))
        .await
        .map_err(|e| Box::new(crate::api::err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))))?;
    Ok(Json(json!({ "app": app, "hidden": b.hidden })).into_response())
}

#[derive(Deserialize)]
struct MemoryQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    limit: Option<usize>,
}

async fn workspace_memory(State(k): K, Member { .. }: Member<at::Guest>, Path(ws): Path<String>, Query(mq): Query<MemoryQuery>) -> ApiResult {
    let limit = mq.limit.unwrap_or(DEFAULT_MEMORY_LIMIT).min(100);
    let items = k.read(|s| s.missions.search_workspace_memory(&ws, &mq.q, limit));
    Ok(Json(items).into_response())
}
