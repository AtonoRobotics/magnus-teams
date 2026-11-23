//! HTTP routes for Team Missions, owner-only Release Airlock actions, Member
//! Capability discovery, and the user avatar.

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
/// Held from a release's check to its event, so two releases of one lane
/// cannot both pass and post it twice.
static RELEASING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

/// Whether `person` may see the room (a channel) a mission lives in.
fn sees_room(k: &Kernel, ws: &str, room: &str, person: &str) -> bool {
    k.read(|s| s.team.workspace(ws).is_some_and(|w| w.sees(room, person)))
}

async fn list_missions(State(k): K, Member { person: p, .. }: Member<at::Guest>, Path((ws, room)): Path<(String, String)>) -> ApiResult {
    if !sees_room(&k, &ws, &room, &p.id) {
        return refuse(StatusCode::NOT_FOUND, "no such channel");
    }
    let view = k.read(|s| s.missions.view_for_room(&ws, &room, &p.id));
    Ok(Json(view).into_response())
}

async fn create_mission(State(k): K, Member { person: p, .. }: Member<at::Member>, Path((ws, room)): Path<(String, String)>, Json(b): Json<CreateMissionBody>) -> ApiResult {
    if !sees_room(&k, &ws, &room, &p.id) {
        return refuse(StatusCode::NOT_FOUND, "no such channel");
    }
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
    let releasing = RELEASING.lock().await;
    let Some((room, scope, objective, draft)) = k.read(|s| {
        let m = s.missions.mission(&mission).filter(|m| m.workspace == ws)?;
        let l = m.lanes.get(&lane)?;
        if l.owner.as_deref() != Some(p.id.as_str()) || l.status == LaneStatus::Released {
            return None;
        }
        Some((m.room.clone(), l.scope.clone(), l.objective.clone(), l.private_draft.clone()))
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
    drop(releasing);

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
    let Some((objective, prev_draft, requester)) = k.read(|s| {
        let m = s.missions.mission(&mission).filter(|m| m.workspace == ws)?;
        let l = m.lanes.get(&lane)?;
        if l.owner.as_deref() != Some(p.id.as_str()) || l.status == LaneStatus::Released {
            return None;
        }
        Some((l.objective.clone(), l.private_draft.clone().unwrap_or_default(), m.requester.clone()))
    }) else {
        return refuse(StatusCode::NOT_FOUND, "no unreleased lane owned by you");
    };

    let actor = format!("person:{}", p.id);
    let prompt = format!("Revise your private draft for \"{objective}\" using the owner's instruction.\n\nCurrent draft:\n{prev_draft}\n\nOwner's edit:\n{instruction}");
    // The objective is the requester's words and the draft came of them: as
    // untrusted as the lane's own run when someone else asked.
    let label = Label { untrusted: requester != p.id, private: true };
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
    let p = who(&k, &h).await.map_err(Box::new)?;
    let actor = format!("person:{}", p.id);
    k.emit(&actor, Ev::TeamMission(TeamMissionEvent::AvatarSet { person: p.id.clone(), avatar: avatar.clone() }))
        .await
        .map_err(|e| Box::new(crate::api::err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))))?;
    Ok(Json(json!({ "avatar": avatar })).into_response())
}
