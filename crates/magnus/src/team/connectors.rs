//! Workspace sign-ins are separate vault and broker principals. A member's
//! personal token is never a fallback for a team request.
use super::routes::{member, Reply, K};
use super::Role;
use crate::api::err;
use crate::kernel::Kernel;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

const FLOW_TTL_SECS: u64 = 600;
struct Flow {
    actor: String,
    workspace: String,
    app: String,
    at: Instant,
}
fn flows() -> &'static tokio::sync::Mutex<HashMap<String, Flow>> {
    static FLOWS: OnceLock<tokio::sync::Mutex<HashMap<String, Flow>>> = OnceLock::new();
    FLOWS.get_or_init(Default::default)
}

pub(crate) fn principal(ws: &str) -> String {
    format!("workspace:{ws}")
}

pub(super) async fn cancel_flows(ws: &str, app: &str) {
    flows().lock().await.retain(|_, f| f.workspace != ws || f.app != app);
}

impl Kernel {
    pub(crate) fn connector_principal(&self, run: &str, fallback: &str) -> String {
        self.read(|s| s.authority(run).and_then(|t| s.team.run_of(&t.task_id)).map(|t| principal(&t.workspace))).unwrap_or_else(|| fallback.to_string())
    }
}

pub(super) fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/workspaces/{ws}/apps", get(list))
        .route("/v1/workspaces/{ws}/apps/{app}/terms", get(terms))
        .route("/v1/workspaces/{ws}/apps/{app}/consent", post(consent))
        .route("/v1/workspaces/{ws}/apps/{app}/sign-in", post(begin))
        .route("/v1/workspaces/{ws}/apps/{app}/sign-in/complete", post(complete))
        .route("/v1/workspaces/{ws}/apps/{app}/disconnect", post(disconnect))
}

fn granted(k: &Kernel, ws: &str, app: &str) -> bool {
    k.read(|s| s.team.workspace(ws).is_some_and(|w| w.connectors.get(app).is_some_and(|r| !r.is_empty())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Ev;
    use crate::team::Change;

    #[tokio::test(flavor = "multi_thread")]
    async fn oauth_completion_rechecks_actor_workspace_grant_and_membership() {
        let s = crate::admin_app::tests::server_with(
            "[[connectors]]\nname='crm'\nurl='https://crm.example/mcp'\ncredential='connector_crm'\nauth='oauth'\naudience='internal'\nlabel='private'\n",
            |_| Router::new(),
        )
        .await;
        let (sam, mo) = (s.token("sam").await, s.token("mo").await);
        for ws in ["a", "b"] {
            s.k.emit("test", Ev::Team(Change::Created { workspace: ws.into(), name: ws.into(), owner: "sam".into() })).await.unwrap();
            s.k.emit("test", Ev::Team(Change::Member { workspace: ws.into(), person: "mo".into(), role: Role::Admin })).await.unwrap();
            s.k.emit("test", Ev::Team(Change::Connector { workspace: ws.into(), app: "crm".into(), roles: vec![Role::Owner, Role::Admin] })).await.unwrap();
        }
        let state = uuid::Uuid::new_v4().to_string();
        flows().lock().await.insert(state.clone(), Flow { actor: "sam".into(), workspace: "a".into(), app: "crm".into(), at: Instant::now() });
        let body = json!({"state":state,"code":"not-exchanged"});
        assert_eq!(s.post(&mo, "/v1/workspaces/a/apps/crm/sign-in/complete", body.clone()).await.0, 400);
        assert_eq!(s.post(&sam, "/v1/workspaces/b/apps/crm/sign-in/complete", body.clone()).await.0, 400);
        assert!(flows().lock().await.contains_key(&state), "another actor consumed the flow");
        s.k.emit("test", Ev::Team(Change::Member { workspace: "a".into(), person: "sam".into(), role: Role::Member })).await.unwrap();
        assert_eq!(s.post(&sam, "/v1/workspaces/a/apps/crm/sign-in/complete", body.clone()).await.0, 403);
        cancel_flows("a", "crm").await;
        assert!(!flows().lock().await.contains_key(&state));
        assert!(s.k.vault.get(&crate::connectors::connection_name("crm", &principal("a"))).unwrap().is_none());
        s.done().await;
    }
}

async fn list(State(k): K, h: HeaderMap, Path(ws): Path<String>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Admin).await);
    let mut cards = vec![];
    for c in k.connectors().into_iter().filter(|c| crate::boundary::may_use(&p, c)) {
        let (standing, _, account) = k.standing(&c, &principal(&ws)).await;
        let roles = k.read(|s| s.team.workspace(&ws).and_then(|w| w.connectors.get(&c.name).cloned())).unwrap_or_default();
        cards.push(json!({"name":c.name, "title":c.title, "state":standing.as_str(), "account":account, "roles":roles,
            "signs_in":c.auth.as_deref()==Some(crate::connectors::OAUTH), "ready":k.provider_ready(&c), "redirect_uri":c.redirect_uri, "handoff":c.handoff}));
    }
    Json(cards).into_response()
}

async fn terms(State(k): K, h: HeaderMap, Path((ws, app)): Path<(String, String)>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Admin).await);
    let Some(c) = k.connector(&app).filter(|c| crate::boundary::may_use(&p, c)) else { return err(StatusCode::NOT_FOUND, "no such app") };
    Json(k.connect_terms(&c, &principal(&ws)).await).into_response()
}

async fn consent(State(k): K, h: HeaderMap, Path((ws, app)): Path<(String, String)>, Json(b): Json<Value>) -> Reply {
    let _files = k.team_files.lock().await;
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Admin).await);
    let Some(c) = k.connector(&app).filter(|c| crate::boundary::may_use(&p, c)) else { return err(StatusCode::NOT_FOUND, "no such app") };
    match k.consent(&c, &principal(&ws), b["terms"].as_str().unwrap_or_default()).await {
        Ok(v) => Json(v).into_response(),
        Err((status, why)) => err(status, why),
    }
}

async fn begin(State(k): K, h: HeaderMap, Path((ws, app)): Path<(String, String)>, Json(b): Json<Value>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Admin).await);
    let Some(c) = k.connector(&app).filter(|c| c.auth.as_deref() == Some(crate::connectors::OAUTH) && crate::boundary::may_use(&p, c)) else {
        return err(StatusCode::NOT_FOUND, "no such OAuth app");
    };
    if !granted(&k, &ws, &app) || k.read(|s| s.kill_switch) {
        return err(StatusCode::CONFLICT, "this workspace cannot connect the app now");
    }
    match k.connector_sign_in(&c, &principal(&ws), b["redirect_uri"].as_str().unwrap_or_default()).await {
        Ok((url, state)) => {
            let mut flows = flows().lock().await;
            flows.retain(|_, f| f.at.elapsed() < Duration::from_secs(FLOW_TTL_SECS));
            flows.insert(state.clone(), Flow { actor: p.id, workspace: ws, app, at: Instant::now() });
            Json(json!({"authorize_url":url,"state":state})).into_response()
        }
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

async fn complete(State(k): K, h: HeaderMap, Path((ws, app)): Path<(String, String)>, Json(b): Json<Value>) -> Reply {
    // Membership and grant revocation cannot race the token exchange and install.
    let _files = k.team_files.lock().await;
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Admin).await);
    let Some(_c) = k.connector(&app).filter(|c| crate::boundary::may_use(&p, c)) else { return err(StatusCode::NOT_FOUND, "no such app") };
    if !granted(&k, &ws, &app) || k.read(|s| s.kill_switch) {
        return err(StatusCode::CONFLICT, "this workspace cannot connect the app now");
    }
    let (Some(state), Some(code)) = (b["state"].as_str(), b["code"].as_str()) else { return err(StatusCode::BAD_REQUEST, "state and code are required") };
    let valid = {
        let mut flows = flows().lock().await;
        let valid = flows.get(state).is_some_and(|f| f.actor == p.id && f.workspace == ws && f.app == app && f.at.elapsed() < Duration::from_secs(FLOW_TTL_SECS));
        if valid {
            flows.remove(state);
        }
        valid
    };
    if !valid {
        return err(StatusCode::BAD_REQUEST, "this sign-in belongs to another request or expired");
    }
    match k.connector_sign_in_complete(&app, &principal(&ws), state, code, b["iss"].as_str()).await {
        Ok(()) => {
            k.signed_in_after_consent(&app, &principal(&ws)).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => err(StatusCode::BAD_REQUEST, format!("{e:#}")),
    }
}

async fn disconnect(State(k): K, h: HeaderMap, Path((ws, app)): Path<(String, String)>) -> Reply {
    let _files = k.team_files.lock().await;
    let _ = ok_or_return!(member(&k, &h, &ws, Role::Admin).await);
    cancel_flows(&ws, &app).await;
    match k.disconnect(&app, &principal(&ws)).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}
