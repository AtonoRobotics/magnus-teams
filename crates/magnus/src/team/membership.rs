//! Who is in a workspace. An owner or admin adds people the server knows and
//! removes members; anyone leaves on their own; nobody changes someone whose
//! role is above theirs; and a workspace always keeps an owner, so its owner
//! goes only by handing ownership on first or by deleting the workspace.
use super::caller::{at, Member};
use super::routes::K;
use super::{Change, Role, Workspace};
use crate::api_error::ApiResult;
use crate::events::Ev;
use crate::kernel::Kernel;
use anyhow::Result;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/workspaces/{ws}/people", get(candidates))
}

/// Why `by`, whose role here is `mine`, may not make `person`'s role `to`
/// (`None` removes them), or nothing when they may. A server admin acts as
/// an owner would. Whether `by` may change other people at all is the
/// route's to say; this is what holds whoever asks.
pub(crate) fn refuse(w: &Workspace, by: &str, mine: Role, person: &str, to: Option<Role>) -> Option<&'static str> {
    let now = w.members.get(person).copied();
    if to.is_some_and(|r| r > mine) {
        return Some("you cannot give a role above your own");
    }
    if person != by && now.is_some_and(|r| r > mine) {
        return Some("you cannot change someone whose role is above yours");
    }
    let owners = w.members.values().filter(|r| **r == Role::Owner).count();
    if now == Some(Role::Owner) && to != Some(Role::Owner) && owners <= 1 {
        return Some("a workspace keeps an owner: make someone else its owner first, or delete the workspace");
    }
    None
}

impl Kernel {
    /// Give `person` a role in the workspace, or take them out of it
    /// (`None`). Taken out, they lose it at once: their running work there
    /// stops, and what was held back for them leaves the workspace for their
    /// own library (their personal space is untouched). What they wrote stays.
    pub(crate) async fn set_workspace_role(&self, actor: &str, ws: &str, person: &str, to: Option<Role>) -> Result<()> {
        let Some(role) = to else {
            let running: Vec<String> = self.read(|s| {
                let mut tasks: Vec<String> = s.team.runs_by(ws, person).into_iter().map(|r| r.task.clone()).collect();
                tasks.extend(s.missions.active_tasks_by(ws, person));
                tasks
            });
            self.keep_held(actor, ws, person).await;
            self.emit(actor, Ev::Team(Change::Removed { workspace: ws.to_string(), person: person.to_string() })).await?;
            self.end_team_tasks(actor, &running).await;
            self.remove_held(ws, person).await;
            return Ok(());
        };
        self.emit(actor, Ev::Team(Change::Member { workspace: ws.to_string(), person: person.to_string(), role })).await?;
        Ok(())
    }

    /// Everyone the server keeps, with their names.
    pub(crate) fn named_people(&self) -> Vec<(String, String)> {
        self.people().all().into_iter().map(|p| (p.id, p.name)).collect()
    }
}

/// The people on this server who are not in the shared project yet, for any
/// collaborator to add: names only, never their addresses.
async fn candidates(State(k): K, _: Member<at::Member>, Path(ws): Path<String>) -> ApiResult {
    let inside: BTreeSet<String> = k.read(|s| s.team.workspace(&ws).map(|w| w.members.keys().cloned().collect()).unwrap_or_default());
    let out: Vec<Value> = k
        .named_people()
        .into_iter()
        .filter(|(id, _)| !inside.contains(id))
        .map(|(id, name)| json!({ "id": id, "name": if name.is_empty() { id.clone() } else { name } }))
        .collect();
    Ok(Json(out).into_response())
}
