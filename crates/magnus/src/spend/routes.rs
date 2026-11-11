//! The app's spend and model routes. A person reads their month and asks
//! for more; what a refused start says is shaped here so the app can offer
//! "Ask for more" where the message appears. The admin handlers take the
//! admin the session names, and are mounted behind the app's admin guard
//! (`admin_app`), which also asks for a recent step-up before a change.

use super::enforce::{details, ModelNotAllowed, SpendLimited};
use super::requests::Refusal;
use super::Scope;
use crate::admin_app::mount::Audit;
use crate::admin_app::Admin;
use crate::api::{err, who};
use crate::kernel::Kernel;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

type K = State<Arc<Kernel>>;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/spend", get(mine)).route("/v1/spend/requests", axum::routing::post(ask))
}

fn reply(r: Result<Value, Refusal>) -> Response {
    match r {
        Ok(v) => Json(v).into_response(),
        Err((code, msg)) => err(code, msg),
    }
}

/// A refused start, as the app reads it: a spent limit (402, with whether
/// the person can ask for more) or a model they may not use (403, with the
/// ones they may).
pub fn refused(e: &anyhow::Error) -> Option<Response> {
    if let Some(s) = e.downcast_ref::<SpendLimited>() {
        return Some((StatusCode::PAYMENT_REQUIRED, Json(json!({ "error": s.to_string(), "spend_limit": details(s) }))).into_response());
    }
    let m = e.downcast_ref::<ModelNotAllowed>()?;
    Some((StatusCode::FORBIDDEN, Json(json!({ "error": m.to_string(), "model": m.model, "allowed_models": m.allowed }))).into_response())
}

async fn mine(State(k): K, h: HeaderMap) -> Response {
    match who(&k, &h).await {
        Ok(p) => Json(k.my_spend(&p.id)).into_response(),
        Err(r) => r,
    }
}

#[derive(Deserialize)]
struct Ask {
    usd: f64,
    #[serde(default)]
    reason: String,
}

async fn ask(State(k): K, h: HeaderMap, Json(b): Json<Ask>) -> Response {
    match who(&k, &h).await {
        Ok(p) => reply(k.ask_for_more(&p.id, b.usd, &b.reason).await),
        Err(r) => r,
    }
}

// Admin handlers: the guard (`admin_app`) let the admin through, with a
// step-up for a change. Each change writes the console's audit entry first.

/// Admin → Spend and Admin → Models, merged into `admin_app::admin_routes`.
/// The providers, their test and the default model are the console's own
/// control routes, mounted with its audit.
pub fn admin_routes() -> Router<Arc<Kernel>> {
    use crate::admin_app::mount::pass;
    Router::new()
        .route("/v1/admin/spend", get(admin_spend))
        .route("/v1/admin/spend/limits", axum::routing::post(admin_set_limit))
        .route("/v1/admin/spend/requests/{id}", axum::routing::post(admin_decide))
        .route("/v1/admin/models", get(admin_models))
        .route("/v1/admin/models/access", axum::routing::post(admin_set_access))
        .route("/v1/admin/providers", get(pass(None)))
        .route("/v1/admin/providers/{id}", axum::routing::patch(pass(Some("provider_set"))))
        .route("/v1/admin/providers/{id}/test", axum::routing::post(pass(None)))
}

type Who = Extension<Admin>;

/// A change, audited as the console audits: the entry first, no change without it.
async fn audited(k: &Arc<Kernel>, a: &Admin, action: &str, target: String, after: Value, change: impl std::future::Future<Output = Response>) -> Response {
    let audit = Audit { action: action.into(), target: Some(target), before: None, after: Some(after) };
    crate::admin_app::mount::audited(k, a, audit, change).await
}

#[derive(Deserialize)]
struct MonthQ {
    #[serde(default)]
    month: Option<String>,
}

/// `GET /v1/admin/spend?month=YYYY-MM`: the limits, the month's spend by
/// person, agent, model and project, the team agent's limits, the requests
/// and the alerts.
async fn admin_spend(State(k): K, Query(q): Query<MonthQ>) -> Response {
    reply(k.spend_overview(q.month.as_deref()))
}

#[derive(Deserialize, serde::Serialize)]
struct LimitB {
    /// `server`, `default`, `person`, `workspace` or `channel`.
    scope: String,
    #[serde(default)]
    person: Option<String>,
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    /// None lifts the limit.
    #[serde(default)]
    usd: Option<f64>,
}

async fn admin_set_limit(State(k): K, Extension(a): Who, Json(b): Json<LimitB>) -> Response {
    let after = json!(b);
    let target =
        format!("spend/{}", [Some(b.scope.as_str()), b.person.as_deref(), b.workspace.as_deref(), b.channel.as_deref()].into_iter().flatten().collect::<Vec<_>>().join("/"));
    let actor = a.actor();
    let change = async {
        reply(match (b.scope.as_str(), b.person, b.workspace) {
            ("server", ..) => k.set_spend_limit(&actor, Scope::Server, b.usd).await,
            ("default", ..) => k.set_spend_limit(&actor, Scope::Default, b.usd).await,
            ("person", Some(id), _) => k.set_spend_limit(&actor, Scope::Person(id), b.usd).await,
            ("workspace", _, Some(ws)) => k.set_team_limit(&actor, &ws, None, b.usd).await,
            ("channel", _, Some(ws)) if b.channel.is_some() => k.set_team_limit(&actor, &ws, b.channel, b.usd).await,
            _ => Err((StatusCode::BAD_REQUEST, "scope is server, default, person (with person), workspace (with workspace) or channel (with workspace and channel)".into())),
        })
    };
    audited(&k, &a, "spend_limit_set", target, after, change).await
}

#[derive(Deserialize, serde::Serialize)]
struct DecideB {
    approve: bool,
    #[serde(default)]
    usd: Option<f64>,
    #[serde(default)]
    note: String,
}

async fn admin_decide(State(k): K, Extension(a): Who, Path(id): Path<String>, Json(b): Json<DecideB>) -> Response {
    let action = if b.approve { "spend_request_approved" } else { "spend_request_declined" };
    let after = json!(b);
    let change = async { reply(k.decide_more(&a.actor(), &id, b.approve, b.usd, &b.note).await) };
    audited(&k, &a, action, format!("spend/requests/{id}"), after, change).await
}

/// `GET /v1/admin/models`: the default model and who may use which, with
/// the roles and people a rule can name. The providers and their models
/// are `GET /v1/admin/providers`.
async fn admin_models(State(k): K) -> Response {
    Json(k.models_overview()).into_response()
}

#[derive(Deserialize, serde::Serialize)]
struct AccessB {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    person: Option<String>,
    /// None removes the rule.
    #[serde(default)]
    models: Option<Vec<String>>,
}

async fn admin_set_access(State(k): K, Extension(a): Who, Json(b): Json<AccessB>) -> Response {
    let target = format!("models/{}", b.person.as_deref().map(|p| format!("person/{p}")).or_else(|| b.role.as_deref().map(|r| format!("role/{r}"))).unwrap_or_default());
    let after = json!(b);
    let change = async { reply(k.set_model_access(&a.actor(), b.role, b.person, b.models).await) };
    audited(&k, &a, "model_access_set", target, after, change).await
}
