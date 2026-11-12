//! Admin › People › Workspaces and Team Admins (the owner, 2026-11-11). A
//! Magnus admin supports the server, not its teams: they see which
//! workspaces there are, how many people and channels each has and who its
//! Team Admins are, and they make someone a Team Admin (who may start
//! workspaces) or take it back. They neither join, change nor delete a
//! workspace, nor read anything in one: that is its Team Admins' to do,
//! from the workspace's own settings.
use super::mount::{audited, Audit};
use super::Admin;
use crate::api::err;
use crate::kernel::Kernel;
use crate::team::Role;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Extension, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) fn routes() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/admin/workspaces", get(list)).route("/v1/admin/people/{id}/team-admin", put(set_team_admin))
}

fn name(k: &Kernel, id: &str) -> String {
    k.person(id).map(|p| p.name).filter(|n| !n.is_empty()).unwrap_or_else(|| id.to_string())
}

/// A workspace as a Magnus admin sees it: its name, its Team Admins, and
/// counts. No channel, member list or content.
fn described(k: &Kernel, ws: &str) -> Option<Value> {
    let w = k.read(|s| s.team.workspace(ws).map(|w| (w.name.clone(), w.members.clone(), w.channels.len())))?;
    let admins: Vec<Value> =
        w.1.iter().filter(|(_, r)| matches!(r, Role::Owner | Role::Admin)).map(|(id, role)| json!({ "id": id, "name": name(k, id), "owner": *role == Role::Owner })).collect();
    Some(json!({ "id": ws, "name": w.0, "team_admins": admins, "members": w.1.len(), "channels": w.2 }))
}

async fn list(State(k): State<Arc<Kernel>>) -> Response {
    let ids: Vec<String> = k.read(|s| s.team.all().map(|w| w.id.clone()).collect());
    let workspaces: Vec<Value> = ids.iter().filter_map(|ws| described(&k, ws)).collect();
    let people: Vec<Value> = k
        .named_people()
        .into_iter()
        .map(|(id, n)| {
            let team_admin = k.person(&id).is_some_and(|p| k.is_team_admin(&p));
            json!({ "id": id, "name": if n.is_empty() { id.clone() } else { n }, "team_admin": team_admin })
        })
        .collect();
    Json(json!({ "workspaces": workspaces, "people": people })).into_response()
}

/// Make someone a Team Admin, or take it back (`{ "on": bool }`). Audited,
/// after the step-up, like every admin change. Taking it back leaves the
/// workspaces they own theirs: it only stops them starting new ones.
async fn set_team_admin(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let id = id.trim().to_lowercase();
    let Some(p) = k.person(&id) else { return err(StatusCode::NOT_FOUND, "no such person") };
    let Some(on) = b["on"].as_bool() else { return err(StatusCode::BAD_REQUEST, "on is true or false") };
    let before = k.is_team_admin(&p);
    let audit = Audit { action: "team_admin_set".into(), target: Some(format!("people/{id}/team-admin")), before: Some(json!(before)), after: Some(json!(on)) };
    audited(&k, &a, audit, async {
        match k.set_team_admin(&a.actor(), &id, on).await {
            Ok(()) => Json(json!({ "id": id, "team_admin": k.person(&id).is_some_and(|p| k.is_team_admin(&p)) })).into_response(),
            Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
        }
    })
    .await
}
