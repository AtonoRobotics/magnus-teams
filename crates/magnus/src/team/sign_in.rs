//! A workspace's own sign-in to an app (spec: "An identity is one account at
//! one site, owned by a person or a workspace. A vault scope is per person
//! or workspace"; "An admin grants a team connector … to the workspace with
//! rules per role. Members use it under their role"). A Team Admin signs an
//! app the workspace was granted in for the workspace; the sign-in is kept
//! in the vault under the workspace's key, never a member's, and only the
//! workspace's team threads act through it. A member's own sign-in still
//! never serves a team thread (`signs_in_per_person`).
use super::caller::{at, Member};
use super::signs_in_per_person;
use crate::api::{err, internal};
use crate::api_error::{refuse, ApiResult};
use crate::config::Connector;
use crate::connectors::requests::{SignIn, SignInComplete};
use crate::connectors::{SignInOwner, OAUTH};
use crate::extract::Body;
use crate::kernel::Kernel;
use crate::state::Run;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;
use std::sync::Arc;

/// The key a workspace's sign-ins are kept under, where a person's id is
/// for a person's (`connectors::connection_name`, `SignInOwner`).
pub(crate) fn owner_key(workspace: &str) -> String {
    SignInOwner::Project(workspace.to_string()).key()
}

/// Whether a sign-in's owner is a workspace, not a person.
pub(crate) fn is_workspace_owner(owner: &str) -> bool {
    matches!(SignInOwner::of_key(owner), SignInOwner::Project(_))
}

impl Kernel {
    /// Whose sign-in a run's call to `c` acts through: in a team thread, an
    /// app people sign in to acts through the workspace's own sign-in;
    /// everywhere else, and for an app with no sign-in, through the run's
    /// person, as before.
    pub(crate) fn app_owner(&self, run: &Run, c: &Connector) -> String {
        match self.team_of(&run.task) {
            Some(t) if signs_in_per_person(c) => owner_key(&t.workspace),
            None if signs_in_per_person(c) && !self.connected(c, &run.person) => self.room_app_workspace(run, c).map_or_else(|| run.person.clone(), |w| owner_key(&w)),
            _ => run.person.clone(),
        }
    }

    /// The shared project whose sign-in to `c` a person's own run asked in
    /// one of its chats acts through, when the person has none of their own:
    /// one that granted `c` to their role and is signed in to it
    /// (2026-11-28-projects-without-hoops). Before, only the retired team
    /// agent's runs used a shared project's apps, so granting one did nothing.
    fn room_app_workspace(&self, run: &Run, c: &Connector) -> Option<String> {
        let ws = self.read(|s| {
            let ws = s.lineage(&run.id).iter().find_map(|t| s.team.whisper_of(t).map(|w| w.workspace.clone()))?;
            let w = s.team.workspace(&ws)?;
            let role = w.members.get(&run.person)?;
            (*role != super::Role::Guest && w.connectors.get(&c.name).is_some_and(|roles| roles.contains(role))).then_some(ws)
        })?;
        self.workspace_signed_in(&ws, c).then_some(ws)
    }

    /// What a team run is told of apps: each the workspace was granted,
    /// with the roles it is open to and whether it works here now, and the
    /// server's other apps, which a Team Admin can grant. So the agent
    /// knows what exists and how the team gets it, and never invents a way.
    pub(crate) fn workspace_apps(&self, workspace: &str, w: &super::Workspace) -> String {
        let all = self.connectors();
        let title = |c: &Connector| c.title.clone().unwrap_or_else(|| crate::connectors::title(&c.name));
        let mut text = String::new();
        let granted: Vec<&Connector> = all.iter().filter(|c| w.connectors.contains_key(&c.name)).collect();
        if !granted.is_empty() {
            text.push_str("\nThe workspace's apps:");
            for c in &granted {
                let roles: Vec<String> = w.connectors[&c.name].iter().map(|r| format!("{r:?}").to_lowercase()).collect();
                let stands = if !signs_in_per_person(c) {
                    "Works here."
                } else if self.workspace_signed_in(workspace, c) {
                    "Connected for the workspace with the team's own account."
                } else {
                    "Not connected for the workspace yet: a Team Admin connects it with the team's account on the workspace's People page."
                };
                text.push_str(&format!("\n- {} ({}), open to {}: {stands}", title(c), c.name, roles.join(", ")));
            }
        }
        let others: Vec<String> = all.iter().filter(|c| !w.connectors.contains_key(&c.name)).map(|c| format!("{} ({})", title(c), c.name)).collect();
        if !others.is_empty() {
            text.push_str(&format!(
                "\nOther apps on this server, not the workspace's: {}. A Team Admin grants one to the workspace on its People page. When a request needs an app the workspace does not have working, say which and how it gets it; a member's own connection never serves a team thread, and there is no other way in.",
                others.join(", ")
            ));
        }
        text
    }

    /// Every app on the server as the workspace's Apps section shows it: the
    /// roles it is granted to (none when it is not the workspace's), whether
    /// it signs in, and whether the workspace is signed in to it.
    pub(crate) fn workspace_app_list(&self, w: &super::Workspace) -> serde_json::Value {
        let apps: Vec<serde_json::Value> = self
            .connectors()
            .iter()
            .map(|c| {
                json!({
                    "name": c.name,
                    "title": c.title.clone().unwrap_or_else(|| crate::connectors::title(&c.name)),
                    "roles": w.connectors.get(&c.name).cloned().unwrap_or_default(),
                    "signs_in": signs_in_per_person(c),
                    "signed_in": signs_in_per_person(c) && self.workspace_signed_in(&w.id, c),
                    "web_sign_in": self.web_sign_in(c),
                    "web_return": self.web_return(c),
                })
            })
            .collect();
        json!(apps)
    }

    /// Whether a workspace holds a sign-in to `c` that the app still accepts.
    pub(crate) fn workspace_signed_in(&self, workspace: &str, c: &Connector) -> bool {
        let owner = owner_key(workspace);
        self.connected(c, &owner) && !self.lapsed(c, &owner)
    }
}

pub(super) fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/workspaces/{ws}/connectors/{name}/sign-in", post(sign_in))
        .route("/v1/workspaces/{ws}/connectors/{name}/sign-in/complete", post(sign_in_complete))
        .route("/v1/workspaces/{ws}/connectors/{name}/connection", axum::routing::delete(disconnect))
}

/// The app, when it is one people sign in to and the workspace was granted it.
fn granted(k: &Kernel, ws: &str, name: &str) -> Result<Connector, &'static str> {
    let c = k.connector(name).filter(|c| c.auth.as_deref() == Some(OAUTH)).ok_or("no app by that name signs in")?;
    if !k.read(|s| s.team.workspace(ws).is_some_and(|w| w.connectors.contains_key(name))) {
        return Err("this app is not granted to the workspace; grant it first");
    }
    Ok(c)
}

/// A Team Admin starts the workspace's sign-in. They sign in with the
/// account the team shares, not their own: what it can reach, every member
/// with a role it is open to can have the agent use.
async fn sign_in(State(k): State<Arc<Kernel>>, _admin: Member<at::Admin>, Path((ws, name)): Path<(String, String)>, Body(b): Body<SignIn>) -> ApiResult {
    let c = match granted(&k, &ws, &name) {
        Ok(c) => c,
        Err(why) => return refuse(StatusCode::NOT_FOUND, why),
    };
    if k.read(|s| s.vault.kill_switch()) {
        return refuse(StatusCode::CONFLICT, "the kill switch is engaged");
    }
    Ok(match k.connector_sign_in(&c, &owner_key(&ws), &b.redirect_uri).await {
        Ok((url, state)) => Json(json!({ "authorize_url": url, "state": state })).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    })
}

/// Finished only for the workspace it was started for, by one of its Team
/// Admins. No conversation opens about it: a workspace has no Magnus chat
/// of its own to open one in.
async fn sign_in_complete(State(k): State<Arc<Kernel>>, _admin: Member<at::Admin>, Path((ws, name)): Path<(String, String)>, Body(b): Body<SignInComplete>) -> ApiResult {
    let (Some(state), Some(code)) = (&b.state, &b.code) else { return refuse(StatusCode::BAD_REQUEST, "state and code are required") };
    if let Err(why) = granted(&k, &ws, &name) {
        return refuse(StatusCode::NOT_FOUND, why);
    }
    Ok(match k.connector_sign_in_complete(&name, &owner_key(&ws), state, code, b.iss.as_deref()).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, format!("{e:#}")),
    })
}

/// A Team Admin ends the workspace's sign-in. Its team threads lose the app
/// at once; members' own sign-ins are untouched. Turned off, an app keeps
/// its sign-ins, so the workspace's can still be ended, as a person's can.
async fn disconnect(State(k): State<Arc<Kernel>>, admin: Member<at::Admin>, Path((ws, name)): Path<(String, String)>) -> ApiResult {
    if !k.all_connectors().iter().any(|(c, _)| c.name == name) {
        return refuse(StatusCode::NOT_FOUND, "no such app");
    }
    Ok(match k.disconnect(&name, &owner_key(&ws), &format!("person:{}", admin.person.id)).await {
        Ok(Some((at_provider, detail))) => Json(json!({ "at_provider": at_provider, "detail": detail })).into_response(),
        Ok(None) => err(StatusCode::NOT_FOUND, "the workspace is not signed in to this app"),
        Err(e) => internal(e),
    })
}

#[cfg(test)]
#[path = "sign_in_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "sign_in_end_tests.rs"]
mod end_tests;
