//! Rules in the Admin section (spec rev 141, Rules): "Workspace rules (allow,
//! ask first or block) that cap every person's own rules, and whether Auto
//! Review acts for the workspace." A workspace rule has the match a
//! person's rule has and is checked as one of each person's
//! (`Kernel::rules_for`), so its block and ask first win over anyone's allow.
//! The policy layers the console edited (draft, test, apply, history) are
//! mounted here too, for the admin who writes Cedar.

use super::mount::{audited, pass, Audit};
use super::people::refuse;
use super::settings::{Change, WorkspaceRule};
use super::Admin;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::rules::Effect;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Extension, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

/// The most workspace rules a server keeps: each is checked on every
/// boundary action of everyone.
pub const WORKSPACE_RULES_MAX: usize = 200;
/// The longest an admin's words for a rule may be.
pub const RULE_WORDS_MAX_CHARS: usize = 500;

pub(super) fn routes() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/admin/rules", get(list).post(add))
        .route("/v1/admin/rules/{id}", put(edit).delete(remove))
        .route("/v1/admin/auto-review", put(auto_review))
        .route("/v1/admin/policy", get(pass(None)))
        .route("/v1/admin/policy/draft", post(pass(Some("policy_drafted"))))
        .route("/v1/admin/policy/apply", post(pass(Some("policy_applied"))))
        .route("/v1/admin/policy/test", post(pass(None)))
        .route("/v1/admin/policy/history", get(pass(None)))
}

async fn list(State(k): State<Arc<Kernel>>) -> Response {
    let (rules, set) = k.read(|s| (s.admin_settings.rules.clone(), s.admin_settings.auto_review));
    let rows: Vec<Value> = rules.into_iter().map(|(id, r)| json!({ "id": id, "rule": r })).collect();
    Json(json!({ "rules": rows, "auto_review": { "acts": k.auto_review_acts(), "set_here": set.is_some(), "config": k.cfg.auto_review.delegate } })).into_response()
}

/// A rule from a request, checked as the boundary will compile it.
fn rule_of(a: &Admin, b: &Value) -> Result<WorkspaceRule, Response> {
    let effect = match b["effect"].as_str() {
        Some("allow") => Effect::Allow,
        Some("ask") => Effect::Ask,
        Some("block") => Effect::Block,
        _ => return Err(refuse(StatusCode::BAD_REQUEST, "invalid", "A rule allows, asks first or blocks.")),
    };
    let text = |f: &str| b[f].as_str().map(str::trim).unwrap_or_default().to_string();
    let destinations: Vec<String> =
        b["destinations"].as_array().into_iter().flatten().filter_map(|d| d.as_str().map(str::trim)).filter(|d| !d.is_empty()).map(String::from).collect();
    let words = b["words"].as_str().map(str::trim).filter(|w| !w.is_empty()).map(String::from);
    if words.as_ref().is_some_and(|w| w.chars().count() > RULE_WORDS_MAX_CHARS) {
        return Err(refuse(StatusCode::BAD_REQUEST, "invalid", "Say the rule in fewer words."));
    }
    let rule = WorkspaceRule { effect, connector: text("connector"), tool: text("tool"), destinations, words, by: a.person.id.clone() };
    // Compiled for someone, as it will be for each person.
    crate::rules::compile(&a.person.id, &rule.connector, &rule.tool, &rule.destinations, false, effect).map_err(|e| refuse(StatusCode::BAD_REQUEST, "invalid", &e))?;
    Ok(rule)
}

async fn set(k: &Arc<Kernel>, a: &Admin, id: String, rule: WorkspaceRule, created: bool) -> Response {
    let before = k.read(|s| s.admin_settings.rules.get(&id).cloned()).map(|r| json!(r));
    let audit = Audit { action: "workspace_rule_set".into(), target: Some(format!("rules/{id}")), before, after: Some(json!(rule)) };
    let change = async {
        match k.emit(&a.actor(), Ev::AdminSetting(Change::Rule { id: id.clone(), rule: rule.clone() })).await {
            Ok(_) => (if created { StatusCode::CREATED } else { StatusCode::OK }, Json(json!({ "id": id, "rule": rule }))).into_response(),
            Err(e) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "failed", &format!("{e:#}")),
        }
    };
    audited(k, a, audit, change).await
}

async fn add(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Json(b): Json<Value>) -> Response {
    let rule = match rule_of(&a, &b) {
        Ok(r) => r,
        Err(r) => return r,
    };
    if k.read(|s| s.admin_settings.rules.len()) >= WORKSPACE_RULES_MAX {
        return refuse(StatusCode::CONFLICT, "too_many", "The workspace has as many rules as it may keep: remove one first.");
    }
    set(&k, &a, uuid::Uuid::new_v4().simple().to_string(), rule, true).await
}

async fn edit(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    if k.read(|s| !s.admin_settings.rules.contains_key(&id)) {
        return refuse(StatusCode::NOT_FOUND, "not_found", "No such rule.");
    }
    match rule_of(&a, &b) {
        Ok(rule) => set(&k, &a, id, rule, false).await,
        Err(r) => r,
    }
}

async fn remove(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(id): Path<String>) -> Response {
    let Some(before) = k.read(|s| s.admin_settings.rules.get(&id).cloned()) else { return refuse(StatusCode::NOT_FOUND, "not_found", "No such rule.") };
    let audit = Audit { action: "workspace_rule_removed".into(), target: Some(format!("rules/{id}")), before: Some(json!(before)), after: None };
    let change = async {
        match k.emit(&a.actor(), Ev::AdminSetting(Change::RuleRemoved { id: id.clone() })).await {
            Ok(_) => StatusCode::NO_CONTENT.into_response(),
            Err(e) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "failed", &format!("{e:#}")),
        }
    };
    audited(&k, &a, audit, change).await
}

/// Whether Auto Review clears an action alone for the workspace. Off, it
/// still reviews every boundary action, and what it would allow asks the
/// person unless a grant or schedule they set up covers it.
async fn auto_review(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Json(b): Json<Value>) -> Response {
    let Some(acts) = b["acts"].as_bool() else { return refuse(StatusCode::BAD_REQUEST, "invalid", "Auto Review acts or does not: true or false.") };
    let audit =
        Audit { action: "auto_review_set".into(), target: Some("auto-review".into()), before: Some(json!({ "acts": k.auto_review_acts() })), after: Some(json!({ "acts": acts })) };
    let change = async {
        match k.emit(&a.actor(), Ev::AdminSetting(Change::AutoReview { acts })).await {
            Ok(_) => Json(json!({ "acts": acts })).into_response(),
            Err(e) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "failed", &format!("{e:#}")),
        }
    };
    audited(&k, &a, audit, change).await
}
