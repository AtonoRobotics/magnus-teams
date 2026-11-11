//! Admin › People › Workspaces: every workspace on the server with its
//! people, for a server admin to add someone the server knows, take a
//! member out, or delete the workspace. Each change is audited like any
//! admin change and needs the step-up; the workspace's own rules still
//! hold (it keeps an owner). A server admin acts as an owner would.
use super::mount::{audited, Audit};
use super::Admin;
use crate::api::err;
use crate::kernel::Kernel;
use crate::team::{membership, Role};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) fn routes() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/admin/workspaces", get(list))
        .route("/v1/admin/workspaces/{ws}", delete(remove))
        .route("/v1/admin/workspaces/{ws}/members", post(set_member))
        .route("/v1/admin/workspaces/{ws}/members/{person}", delete(remove_member))
}

fn name(k: &Kernel, id: &str) -> String {
    k.person(id).map(|p| p.name).filter(|n| !n.is_empty()).unwrap_or_else(|| id.to_string())
}

/// What the audit records of a workspace: its name and who was in it.
fn described(k: &Kernel, ws: &str) -> Option<Value> {
    let w = k.read(|s| s.team.workspace(ws).map(|w| (w.name.clone(), w.members.clone(), w.channels.len())))?;
    let members: Vec<Value> = w.1.iter().map(|(id, role)| json!({ "id": id, "name": name(k, id), "role": role })).collect();
    Some(json!({ "id": ws, "name": w.0, "members": members, "channels": w.2 }))
}

async fn list(State(k): State<Arc<Kernel>>) -> Response {
    let ids: Vec<String> = k.read(|s| s.team.all().map(|w| w.id.clone()).collect());
    let workspaces: Vec<Value> = ids.iter().filter_map(|ws| described(&k, ws)).collect();
    let people: Vec<Value> = k.named_people().into_iter().map(|(id, n)| json!({ "id": id, "name": if n.is_empty() { id.clone() } else { n } })).collect();
    Json(json!({ "workspaces": workspaces, "people": people })).into_response()
}

fn missing() -> Response {
    err(StatusCode::NOT_FOUND, "no such workspace")
}

/// Delete a workspace and everything in it, as its owner can.
async fn remove(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(ws): Path<String>) -> Response {
    let Some(before) = described(&k, &ws) else { return missing() };
    let audit = Audit { action: "workspace_deleted".into(), target: Some(format!("workspaces/{ws}")), before: Some(before), after: None };
    audited(&k, &a, audit, async {
        match k.delete_workspace(&a.actor(), &ws).await {
            Ok(()) => Json(json!({ "ok": true })).into_response(),
            Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
        }
    })
    .await
}

#[derive(Deserialize)]
struct SetMember {
    person: String,
    role: Role,
}

async fn set_member(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(ws): Path<String>, Json(b): Json<SetMember>) -> Response {
    if k.person(&b.person).is_none() {
        return err(StatusCode::NOT_FOUND, "no such person");
    }
    let before = k.read(|s| s.team.role(&ws, &b.person));
    let audit =
        Audit { action: "workspace_member_set".into(), target: Some(format!("workspaces/{ws}/members/{}", b.person)), before: Some(json!(before)), after: Some(json!(b.role)) };
    change(&k, &a, &ws, &b.person, Some(b.role), audit).await
}

async fn remove_member(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path((ws, person)): Path<(String, String)>) -> Response {
    let Some(before) = k.read(|s| s.team.role(&ws, &person)) else { return err(StatusCode::NOT_FOUND, "they are not in this workspace") };
    let audit = Audit { action: "workspace_member_removed".into(), target: Some(format!("workspaces/{ws}/members/{person}")), before: Some(json!(before)), after: None };
    change(&k, &a, &ws, &person, None, audit).await
}

async fn change(k: &Arc<Kernel>, a: &Admin, ws: &str, person: &str, to: Option<Role>, audit: Audit) -> Response {
    let why = k.read(|s| s.team.workspace(ws).map(|w| membership::refuse(w, &a.person.id, Role::Owner, person, to)));
    match why {
        None => return missing(),
        Some(Some(why)) => return err(StatusCode::FORBIDDEN, why),
        Some(None) => {}
    }
    audited(k, a, audit, async {
        match k.set_workspace_role(&a.actor(), ws, person, to).await {
            Ok(()) => Json(json!({ "ok": true })).into_response(),
            Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
        }
    })
    .await
}
