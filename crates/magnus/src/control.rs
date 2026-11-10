//! The kernel's control socket (admin console spec, Server and API): how
//! the admin console reaches the kernel for what only the kernel may do,
//! a local Unix socket in the appliance's state, readable and writable by
//! the appliance's own user alone. Nothing reaches it over the network.
//!
//! Its first use is the audit log: every console action is written to the
//! event log by the kernel, its one writer, before the console answers.

use crate::events::Ev;
use crate::kernel::Kernel;
use anyhow::{Context, Result};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn socket_path(state_dir: &Path) -> PathBuf {
    state_dir.join("control.sock")
}

/// Serve the control socket until the kernel stops.
/// Listen on the control socket, replacing one a previous process left.
/// Bound before the appliance answers on the network, so nothing that has
/// seen it answer finds the old socket (connection refused) instead.
pub fn bind(state_dir: &Path) -> Result<tokio::net::UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    let path = socket_path(state_dir);
    let _ = std::fs::remove_file(&path);
    let l = tokio::net::UnixListener::bind(&path).with_context(|| format!("listen on {}", path.display()))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    tracing::info!("magnus control socket at {}", path.display());
    Ok(l)
}

pub async fn serve(k: Arc<Kernel>, l: tokio::net::UnixListener) -> Result<()> {
    let app = Router::new()
        .route("/v1/audit", post(record).get(list))
        .route("/v1/kill-switch", post(switch).get(switch_state))
        .route("/v1/overview", axum::routing::get(overview))
        .route("/v1/people", axum::routing::get(people))
        .route("/v1/people/{person}/sessions", axum::routing::get(sessions).delete(end_sessions))
        .route("/v1/people/{person}/sessions/{id}", axum::routing::delete(end_session))
        .route("/v1/admin/people", axum::routing::get(admin_people).post(admin_invite))
        .route("/v1/admin/people/{id}", axum::routing::get(admin_person).patch(admin_patch_person).delete(admin_delete))
        .route("/v1/admin/people/{id}/disable", post(admin_disable))
        .route("/v1/admin/people/{id}/enable", post(admin_enable))
        .route("/v1/admin/people/{id}/cancel-invite", post(admin_cancel_invite))
        .route("/v1/admin/people/{id}/invite", post(admin_resend_invite))
        .route("/v1/admin/people/{id}/invite/password", post(admin_invite_password))
        .route("/v1/admin/sign-in-policy", axum::routing::get(sign_in_policy).put(set_sign_in_policy))
        .route("/v1/people/{person}/schedules", axum::routing::get(person_schedules))
        .route("/v1/people/{person}/schedules/{id}/{change}", post(person_schedule))
        .route("/v1/people/{person}/data/{what}", post(person_data))
        .route("/v1/people/{person}/erase", post(erase_person))
        .route("/v1/admin/policy", axum::routing::get(policy_sets))
        .route("/v1/admin/policy/draft", post(policy_draft))
        .route("/v1/admin/policy/apply", post(policy_apply))
        .route("/v1/admin/policy/test", post(policy_test))
        .route("/v1/admin/policy/history", axum::routing::get(policy_history))
        .route("/v1/admin/console-settings", axum::routing::get(console_settings).put(set_console_settings))
        .route("/v1/admin/modules", axum::routing::get(modules))
        .route("/v1/admin/modules/{name}", post(set_module))
        .route("/v1/admin/providers", axum::routing::get(providers))
        .route("/v1/admin/providers/{id}", axum::routing::patch(set_provider))
        .route("/v1/admin/providers/{id}/test", post(test_provider))
        .route("/v1/admin/updates", axum::routing::get(updates))
        .route("/v1/admin/updates/install", post(install_update))
        .route("/v1/admin/updates/rollback", post(roll_back_update))
        .route("/v1/admin/updates/desktop-channel", axum::routing::put(desktop_channel))
        .route("/v1/admin/knowledge", axum::routing::get(knowledge_list))
        .route("/v1/admin/knowledge/doc", axum::routing::get(knowledge_doc).post(knowledge_write).delete(knowledge_remove))
        .route("/v1/admin/knowledge/version", axum::routing::get(knowledge_version))
        .route("/v1/admin/knowledge/rollback", post(knowledge_rollback))
        .route("/v1/admin/knowledge/merge", post(knowledge_merge))
        .route("/v1/admin/knowledge/activity", axum::routing::get(knowledge_activity))
        .route("/v1/admin/knowledge/activity/{seq}", axum::routing::get(knowledge_change))
        .route("/v1/admin/knowledge/revert", post(knowledge_revert))
        .route("/v1/admin/knowledge/assembled", axum::routing::get(knowledge_assembled))
        .route("/v1/admin/connectors", axum::routing::get(connectors).post(add_connector))
        .route("/v1/admin/connectors/{name}", axum::routing::patch(connector_settings).delete(remove_connector))
        .merge(crate::registrations::routes())
        .route("/v1/admin/console-sign-in", post(console_sign_in))
        .route("/v1/admin/console-standing/{person}", axum::routing::get(console_standing))
        .with_state(k);
    axum::serve(l, app).await?;
    Ok(())
}

fn text(b: &Value, f: &str) -> Option<String> {
    b[f].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// Record a console action; its seq once it is in the log.
async fn record(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let (Some(admin), Some(action)) = (text(&b, "admin"), text(&b, "action")) else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "an audit entry names its admin and action" }))).into_response();
    };
    let ev = Ev::AdminAction {
        admin: admin.clone(),
        name: text(&b, "name").unwrap_or_default(),
        action,
        target: text(&b, "target"),
        source: text(&b, "source"),
        before: b.get("before").filter(|v| !v.is_null()).cloned(),
        after: b.get("after").filter(|v| !v.is_null()).cloned(),
    };
    match k.emit(&format!("admin:{admin}"), ev).await {
        Ok(e) => Json(json!({ "seq": e.seq })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response(),
    }
}

/// The audit, newest first, filtered by actor, action, target and time.
async fn list(State(k): State<Arc<Kernel>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let since = q.get("since").and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());
    let until = q.get("until").and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());
    let has = |v: &Value, f: &str, want: Option<&String>| want.is_none_or(|w| v[f].as_str().is_some_and(|x| x.to_lowercase().contains(&w.to_lowercase())));
    let rows: Vec<Value> = k.read(|s| {
        s.audit
            .iter()
            .rev()
            .filter(|(_, at, _)| since.is_none_or(|t| *at >= t) && until.is_none_or(|t| *at <= t))
            .filter(|(_, _, v)| (has(v, "admin", q.get("actor")) || has(v, "name", q.get("actor"))) && has(v, "action", q.get("action")) && has(v, "target", q.get("target")))
            .take(1000)
            .map(|(seq, at, v)| {
                let mut r = v.clone();
                r["seq"] = json!(seq);
                r["at"] = json!(at);
                r
            })
            .collect()
    });
    Json(json!({ "entries": rows })).into_response()
}

/// The kill switch as the console shows it: on or off, who moved it and
/// when, and what it holds stopped (the runs it suspended, the schedules
/// that do not fire while it is on).
async fn switch_state(State(k): State<Arc<Kernel>>) -> Response {
    let stopped: Vec<String> =
        k.read(|s| s.runs.values().filter(|r| r.status == crate::state::Status::Suspended && r.reason.as_deref() == Some("kill_switch")).map(|r| r.id.clone()).collect());
    let frozen = stopped.iter().filter(|r| k.sandboxes.kept(r)).count();
    let running = k.sandboxes.running().await;
    Json(k.read(|s| {
        let halted = s.runs.values().filter(|r| r.status == crate::state::Status::Suspended && r.reason.as_deref() == Some("kill_switch")).count();
        let held = if s.kill_switch { s.schedules.values().filter(|x| x.next_due().is_some()).count() } else { 0 };
        json!({
            "engaged": s.kill_switch,
            "by": s.kill_switch_by.as_ref().map(|(who, _)| who), "at": s.kill_switch_by.as_ref().map(|(_, at)| at),
            "tasks_halted": halted, "schedules_held": held,
            "sign_ins_revoked": s.revoked.len(),
            "microvms_frozen": frozen, "microvms_running": running,
        })
    }))
    .into_response()
}

/// Engage or release it, as `actor`: the one switch, the app's own.
async fn switch(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let Some(actor) = text(&b, "actor") else { return (StatusCode::BAD_REQUEST, Json(json!({ "error": "name who moves the switch" }))).into_response() };
    match k.kill_switch(&actor, b["engaged"].as_bool().unwrap_or(true)).await {
        Ok(v) => Json(v).into_response(),
        Err((st, v)) => (st, Json(v)).into_response(),
    }
}

/// What the console's Overview and Health show of the kernel: its version,
/// whether the log answers, the kill switch, tasks running, people signed
/// in, what agents changed in the organisation's knowledge and memory in
/// the last day, and the runs that failed most recently.
async fn overview(State(k): State<Arc<Kernel>>) -> Response {
    use crate::state::Status;
    let log_up = k.log.up().await;
    let signed_in = k.accounts.signed_in().await.len();
    let vms = k.sandboxes.running().await;
    let day = chrono::Utc::now() - chrono::Duration::hours(24);
    let v = k.read(|s| {
        let mut failed: Vec<&crate::state::Run> = s.runs.values().filter(|r| r.status == Status::Failed).collect();
        failed.sort_by(|a, b| b.ended_at.cmp(&a.ended_at));
        let knowledge = s.knowledge.values().filter(|e| e.at > day && !e.author.starts_with("person:")).count();
        let memory = s.memory.iter().filter(|m| m.at > day).count();
        // Agents' changes to the org context bundle (admin console spec,
        // Overview: "agent knowledge changes in the last 24 hours").
        let bundle = s.bundle.values().filter(|d| d.scope == "org").flat_map(|d| d.versions.iter()).filter(|v| v.2 > day && !v.3.starts_with("human:")).count();
        let last_change = s.bundle.values().filter(|d| d.scope == "org").flat_map(|d| d.versions.iter()).map(|v| v.1).max();
        json!({
            "version": crate::update::version(), "log_up": log_up, "kill_switch": s.kill_switch,
            "running_tasks": s.runs.values().filter(|r| r.status == Status::Running).count(),
            "waiting_tasks": s.runs.values().filter(|r| r.status == Status::Suspended).count(),
            "signed_in": signed_in, "microvms_running": vms,
            "knowledge_changes_24h": knowledge + memory + bundle, "knowledge_last_change": last_change,
            "recent_errors": failed.iter().take(10).map(|r| json!({ "run": r.id, "task": r.task, "person": r.person, "reason": r.reason, "at": r.ended_at })).collect::<Vec<_>>(),
        })
    });
    Json(v).into_response()
}

/// The person here whom the identity provider knows as `sub` (owner's
/// ruling 3: people are bound to their provider's subject at their first
/// sign-in through it). The console names people by their `sub`.
/// A console path id to a person: a provider `sub` first, while the
/// provider still binds people, else the id itself when it is a person here
/// (#508: the console now addresses local ids).
fn person_of(k: &Kernel, sub: &str) -> Option<String> {
    k.read(|s| s.subjects.iter().find(|(_, v)| v.as_str() == sub).map(|(p, _)| p.clone())).or_else(|| k.person(sub).map(|p| p.id.clone()))
}

/// Each person known here, by their provider `sub`: when they last signed
/// in, and how many sessions they hold open (the console's Users list; the
/// people themselves are the identity provider's).
async fn people(State(k): State<Arc<Kernel>>) -> Response {
    let bound: Vec<(String, String, Option<chrono::DateTime<chrono::Utc>>)> =
        k.read(|s| s.subjects.iter().map(|(p, sub)| (p.clone(), sub.clone(), s.last_sign_in.get(p).copied())).collect());
    let mut out = serde_json::Map::new();
    for (p, sub, at) in bound {
        out.insert(sub, json!({ "last_sign_in": at, "sessions": k.accounts.sessions_of(&p, "").await.len() }));
    }
    Json(json!({ "people": out })).into_response()
}

/// Everyone the appliance keeps — config people and local ones — with the
/// role, status, last sign-in and open sessions the console's Users list
/// shows (#508). The realm directory is no longer read for this.
async fn admin_people(State(k): State<Arc<Kernel>>) -> Response {
    let mut ids: std::collections::BTreeSet<String> = k.cfg.people.iter().map(|p| p.id.clone()).collect();
    ids.extend(k.accounts.local_people().into_iter().map(|(id, _)| id));
    let mut rows = Vec::new();
    for id in ids {
        rows.push(admin_person_row(&k, &id).await);
    }
    Json(json!({ "people": rows })).into_response()
}

async fn admin_person_row(k: &Kernel, id: &str) -> Value {
    let p = k.person(id);
    let email = k.accounts.local_person(id).and_then(|l| l.email);
    let (role, status) = tokio::join!(k.accounts.role(id), k.accounts.status(id));
    let at = k.read(|s| s.last_sign_in.get(id).copied());
    let sessions = k.accounts.sessions_of(id, "").await.len();
    json!({
        "id": id,
        "name": p.map(|p| if p.name.is_empty() { id.to_string() } else { p.name }).unwrap_or_else(|| id.to_string()),
        "email": email,
        "username": id,
        "role": match role { crate::accounts::Role::Admin => "admin", crate::accounts::Role::Member => "member" },
        "status": match status { crate::accounts::Status::Invited => "invited", crate::accounts::Status::Active => "active", crate::accounts::Status::Disabled => "disabled" },
        "created": Value::Null,
        "last_sign_in": at,
        "sessions": sessions,
    })
}

/// One person the appliance keeps, by their id.
async fn admin_person(State(k): State<Arc<Kernel>>, axum::extract::Path(id): axum::extract::Path<String>) -> Response {
    let id = id.trim().to_lowercase();
    if k.person(&id).is_none() {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response();
    }
    Json(admin_person_row(&k, &id).await).into_response()
}

fn last_admin_refused(e: &anyhow::Error) -> bool {
    format!("{e:#}").contains("last admin")
}

fn last_admin() -> Response {
    (StatusCode::CONFLICT, Json(json!({ "error": "last_admin", "message": "This is the organisation's last admin: make someone else an admin first." }))).into_response()
}

/// Change a role, a status, or both. Ending sessions are recorded, so the
/// person's devices sign out on the record.
async fn admin_patch_person(State(k): State<Arc<Kernel>>, axum::extract::Path(id): axum::extract::Path<String>, Json(b): Json<Value>) -> Response {
    let id = id.trim().to_lowercase();
    if k.person(&id).is_none() {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response();
    }
    if let Some(role) = b["role"].as_str() {
        let role = match role {
            "admin" => crate::accounts::Role::Admin,
            "member" => crate::accounts::Role::Member,
            _ => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid", "message": "The role is admin or member." }))).into_response(),
        };
        if let Err(e) = k.accounts.set_role(&id, role).await {
            if last_admin_refused(&e) {
                return last_admin();
            }
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response();
        }
    }
    if let Some(status) = b["status"].as_str() {
        let status = match status {
            "active" => crate::accounts::Status::Active,
            "disabled" => crate::accounts::Status::Disabled,
            _ => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid", "message": "The status is active or disabled." }))).into_response(),
        };
        let Some(actor) = actor(&b) else { return (StatusCode::BAD_REQUEST, Json(json!({ "error": "name who changes them" }))).into_response() };
        match k.accounts.set_status(&id, status).await {
            Ok(ended) => {
                let evs: Vec<(&str, Ev)> = ended.into_iter().map(|session| (actor.as_str(), Ev::SignedOut { person: id.clone(), session })).collect();
                if !evs.is_empty() {
                    if let Err(e) = k.emit_all(evs).await {
                        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response();
                    }
                }
            }
            Err(e) => {
                if last_admin_refused(&e) {
                    return last_admin();
                }
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response();
            }
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

/// Turn someone off: no sign-in, their sessions ended on the record, their
/// schedules paused. Never the last admin.
async fn admin_disable(State(k): State<Arc<Kernel>>, axum::extract::Path(id): axum::extract::Path<String>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return (StatusCode::BAD_REQUEST, Json(json!({ "error": "name who turns them off" }))).into_response() };
    let id = id.trim().to_lowercase();
    if k.person(&id).is_none() {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response();
    }
    let due = k.read(|s| s.schedules.values().filter(|x| x.person == id && x.next_due().is_some()).count());
    let ended = match k.accounts.set_status(&id, crate::accounts::Status::Disabled).await {
        Ok(ended) => ended,
        Err(e) => {
            if last_admin_refused(&e) {
                return last_admin();
            }
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response();
        }
    };
    let evs: Vec<(&str, Ev)> = ended.into_iter().map(|session| (actor.as_str(), Ev::SignedOut { person: id.clone(), session })).collect();
    let sessions = evs.len();
    if !evs.is_empty() {
        if let Err(e) = k.emit_all(evs).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response();
        }
    }
    k.turn_off(&id, &format!("{actor} turned {id} off in the admin console"), "An administrator has turned your account off").await;
    Json(json!({ "sessions_ended": sessions, "schedules_paused": due })).into_response()
}

/// Turn them back on. Their paused schedules stay paused until they resume
/// them; the turned-off mark clears when they sign in again.
async fn admin_enable(State(k): State<Arc<Kernel>>, axum::extract::Path(id): axum::extract::Path<String>) -> Response {
    let id = id.trim().to_lowercase();
    if k.person(&id).is_none() {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response();
    }
    match k.accounts.set_status(&id, crate::accounts::Status::Active).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response(),
    }
}

/// Delete them: their content erased, then their local account where the
/// appliance keeps one. A config person cannot be unmade here, so they are
/// turned off instead and stay listed, disabled.
async fn admin_delete(State(k): State<Arc<Kernel>>, axum::extract::Path(id): axum::extract::Path<String>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return (StatusCode::BAD_REQUEST, Json(json!({ "error": "name who deletes them" }))).into_response() };
    let id = id.trim().to_lowercase();
    if k.person(&id).is_none() {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response();
    }
    // Disable first: it holds the last-admin guard, so a refused
    // deletion erases nothing — and a failed erasure leaves a listed,
    // disabled account whose deletion retries cleanly, instead of a
    // vanished person with orphaned content (#529).
    let local = k.accounts.is_local(&id);
    let ended = match k.accounts.set_status(&id, crate::accounts::Status::Disabled).await {
        Ok(ended) => ended,
        Err(e) if last_admin_refused(&e) => return last_admin(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response(),
    };
    // Sessions the disabling ended are signed out on the record, as a
    // disable signs its own.
    if !ended.is_empty() {
        let evs: Vec<(&str, Ev)> = ended.iter().map(|session| (actor.as_str(), Ev::SignedOut { person: id.clone(), session: session.clone() })).collect();
        if let Err(e) = k.emit_all(evs).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response();
        }
    }
    let mut erased = match k.erase_person(&actor, &id).await {
        Ok(v) => v,
        Err(r) => return person_refused(r),
    };
    // The disabling already ended their sessions; the erasure counts
    // its own. The answer counts every session the deletion ended.
    erased["sessions_ended"] = json!(ended.len() as u64 + erased["sessions_ended"].as_u64().unwrap_or(0));
    // Then the local directory entry goes, its content already erased. A
    // disabled account is never the last enabled admin, so this only
    // fails the store itself — leaving a listed, disabled shell whose
    // deletion retries (the erasure finds nothing new) and removes.
    if local {
        if let Err(e) = k.accounts.remove_person(&id).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response();
        }
    }
    Json(json!({ "erased": erased, "account_removed": local })).into_response()
}

/// Withdraw an invitation sent from here: the local account goes, never
/// having held anything. Only a local invited person can be withdrawn.
async fn admin_cancel_invite(State(k): State<Arc<Kernel>>, axum::extract::Path(id): axum::extract::Path<String>) -> Response {
    let id = id.trim().to_lowercase();
    if k.person(&id).is_none() {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response();
    }
    if k.accounts.status(&id).await != crate::accounts::Status::Invited || !k.accounts.is_local(&id) {
        return (StatusCode::CONFLICT, Json(json!({ "error": "not_invited", "message": "Only an invitation sent from here can be withdrawn." }))).into_response();
    }
    match k.accounts.remove_person(&id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            if last_admin_refused(&e) {
                return last_admin();
            }
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response()
        }
    }
}

/// Invite someone: the appliance makes their local account and a
/// single-use token, shown once here for the admin to pass on — the
/// appliance cannot send mail, so there is no email step (#508). The id
/// derives from the email's local part; a held id refuses `taken`,
/// never a surprising sibling.
async fn admin_invite(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let email = b["email"].as_str().map(str::trim).unwrap_or_default().to_lowercase();
    let name = b["name"].as_str().map(str::trim).unwrap_or_default();
    let role = match b["role"].as_str() {
        Some("admin") => crate::accounts::Role::Admin,
        Some("member") => crate::accounts::Role::Member,
        _ => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid", "message": "The role is admin or member." }))).into_response(),
    };
    if !invite_email(&email) || name.is_empty() || name.len() > 200 {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid", "message": "An invitation needs an email address and a name." }))).into_response();
    }
    let id: String = email.split('@').next().unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_' || *c == '-').collect();
    if id.is_empty() || id.len() > 64 {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid", "message": "No person id derives from that address." }))).into_response();
    }
    if k.person(&id).is_some() {
        return (StatusCode::CONFLICT, Json(json!({ "error": "taken", "message": format!("{id} is already a person here.") }))).into_response();
    }
    match k.accounts.invite_person(&id, name, Some(&email), role).await {
        Ok(token) => (
            StatusCode::CREATED,
            Json(json!({ "id": id, "email": email, "invite_token": token, "invite_expires": chrono::Utc::now() + chrono::Duration::days(crate::accounts::INVITE_DAYS) })),
        )
            .into_response(),
        Err(e) => {
            let message = format!("{e:#}");
            if message.contains("already a person") {
                return (StatusCode::CONFLICT, Json(json!({ "error": "taken", "message": message }))).into_response();
            }
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": message }))).into_response()
        }
    }
}

fn invite_email(e: &str) -> bool {
    let Some((user, host)) = e.split_once('@') else { return false };
    !user.is_empty()
        && host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && e.len() <= 254
        && !e.chars().any(|c| c.is_whitespace() || c == '<' || c == '>' || c == ',')
}

/// Send the invitation again: a fresh token, shown once here; the old
/// one dies with it. Only an invitation still waiting.
async fn admin_resend_invite(State(k): State<Arc<Kernel>>, axum::extract::Path(id): axum::extract::Path<String>) -> Response {
    let id = id.trim().to_lowercase();
    if k.person(&id).is_none() {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response();
    }
    if k.accounts.status(&id).await != crate::accounts::Status::Invited {
        return (StatusCode::CONFLICT, Json(json!({ "error": "not_invited", "message": "They have already chosen their password: this is an account, not an invitation." })))
            .into_response();
    }
    match k.accounts.invite(&id, None).await {
        Ok(token) => Json(json!({ "id": id, "invite_token": token, "invite_expires": chrono::Utc::now() + chrono::Duration::days(crate::accounts::INVITE_DAYS) })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response(),
    }
}

/// Give an invited person a password to choose with once, shown to the
/// admin here only. It never signs in; choosing with it sets their own.
async fn admin_invite_password(State(k): State<Arc<Kernel>>, axum::extract::Path(id): axum::extract::Path<String>, Json(b): Json<Value>) -> Response {
    let id = id.trim().to_lowercase();
    if k.person(&id).is_none() {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response();
    }
    if k.accounts.status(&id).await != crate::accounts::Status::Invited {
        return (StatusCode::CONFLICT, Json(json!({ "error": "not_invited", "message": "They have already chosen their password: this is an account, not an invitation." })))
            .into_response();
    }
    let password = b["password"].as_str().unwrap_or_default();
    match k.accounts.set_temporary_password(&id, password).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            let message = format!("{e:#}");
            if message.contains("not an invitation") {
                return (
                    StatusCode::CONFLICT,
                    Json(json!({ "error": "not_invited", "message": "They have already chosen their password: this is an account, not an invitation." })),
                )
                    .into_response();
            }
            (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid", "message": message }))).into_response()
        }
    }
}

/// The sign-in rules the console shows and the kernel enforces on the
/// local path (#508).
async fn sign_in_policy(State(k): State<Arc<Kernel>>) -> Response {
    match k.accounts.policy().await {
        Ok(p) => Json(json!(p)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response(),
    }
}

/// Change the sign-in rules: only what is given changes. The longest
/// wait cannot be shorter than the first, against the merged rules.
async fn set_sign_in_policy(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let mut p = match k.accounts.policy().await {
        Ok(p) => p,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response(),
    };
    let invalid = |message: String| (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid", "message": message }))).into_response();
    let ranged = |v: &Value, range: std::ops::RangeInclusive<i64>, what: &str| match v {
        Value::Null => Ok(None),
        x => x.as_i64().filter(|n| range.contains(n)).map(Some).ok_or_else(|| format!("{what} is a whole number from {} to {}.", range.start(), range.end())),
    };
    if let Some(required) = b["code_required"].as_bool() {
        p.code_required = required;
    } else if !b["code_required"].is_null() && b.get("code_required").is_some() {
        return invalid("Whether a code is required is true or false.".into());
    }
    let apply = (|| -> Result<(), String> {
        if let Some(n) = ranged(&b["idle_minutes"], 5..=1440, "Members' idle time")? {
            p.idle_minutes = n;
        }
        if let Some(n) = ranged(&b["max_hours"], 1..=720, "Members' longest session")? {
            p.max_hours = n;
        }
        if let Some(n) = ranged(&b["lockout_failures"], 1..=100, "Failures before a lockout")? {
            p.lockout_failures = n;
        }
        if let Some(n) = ranged(&b["lockout_wait_minutes"], 1..=1440, "The first wait")? {
            p.lockout_wait_minutes = n;
        }
        if let Some(n) = ranged(&b["lockout_max_wait_minutes"], 1..=10080, "The longest wait")? {
            p.lockout_max_wait_minutes = n;
        }
        Ok(())
    })();
    if let Err(message) = apply {
        return invalid(message);
    }
    for (field, name) in [(b.get("lockout_enabled"), "lockout_enabled"), (b.get("lockout_permanent"), "lockout_permanent")] {
        match field {
            None | Some(Value::Null) => {}
            Some(Value::Bool(on)) => {
                if name == "lockout_enabled" {
                    p.lockout_enabled = *on;
                } else {
                    p.lockout_permanent = *on;
                }
            }
            Some(_) => return invalid(format!("{name} is true or false.")),
        }
    }
    if p.lockout_max_wait_minutes < p.lockout_wait_minutes {
        return invalid("The longest wait cannot be shorter than the first.".into());
    }
    match k.accounts.set_policy(p).await {
        Ok(_) => Json(json!(p)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response(),
    }
}

/// A person's open sessions here, newest first: each a device signed in.
async fn sessions(State(k): State<Arc<Kernel>>, axum::extract::Path(sub): axum::extract::Path<String>) -> Response {
    let sessions = match person_of(&k, &sub) {
        Some(p) => k.accounts.sessions_of(&p, "").await,
        None => vec![],
    };
    let list: Vec<Value> = sessions.into_iter().map(|(s, _)| json!({ "id": s.id, "client": s.client, "created": s.created, "expires": s.expires })).collect();
    Json(json!({ "sessions": list })).into_response()
}

/// Check an admin's local console sign-in: password and code against the
/// local store, on the console's own lockout counter. Answers ok only for
/// an active admin; everyone else refuses alike, and a right password with
/// a code still to come asks for it. Always 200: the verdict is data.
/// Only the console calls this, over the control socket (#429).
async fn console_sign_in(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let person = b["person"].as_str().unwrap_or_default().trim().to_lowercase();
    let password = b["password"].as_str().unwrap_or_default();
    let code = b["code"].as_str().map(str::trim).filter(|c| !c.is_empty());
    let name = match k.person(&person) {
        Some(p) => {
            if p.name.is_empty() {
                person.clone()
            } else {
                p.name.clone()
            }
        }
        None => {
            let _ = k.accounts.sign_in(&person, password).await;
            return Json(json!({ "result": "refused" })).into_response();
        }
    };
    let counter = crate::accounts::Accounts::console_counter(&person);
    let admin = k.accounts.role(&person).await == crate::accounts::Role::Admin;
    match k.accounts.sign_in_counted_with_code(&person, password, code, &counter).await {
        Ok(Ok(_)) if admin => Json(json!({ "result": "ok", "name": name })).into_response(),
        Ok(Err(crate::accounts::SignInError::NeedsCode)) if admin => Json(json!({ "result": "needs_code" })).into_response(),
        Ok(Err(crate::accounts::SignInError::Locked(until))) => Json(json!({ "result": "locked", "retry_at": until })).into_response(),
        Ok(_) => Json(json!({ "result": "refused" })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response(),
    }
}

/// Whether `person` still holds the console: here, active and an admin.
/// The guard asks on its refresh round (#429).
async fn console_standing(State(k): State<Arc<Kernel>>, axum::extract::Path(person): axum::extract::Path<String>) -> Response {
    let ok = k.person(&person).is_some() && k.accounts.status(&person).await == crate::accounts::Status::Active && k.accounts.role(&person).await == crate::accounts::Role::Admin;
    Json(json!({ "ok": ok })).into_response()
}

pub(crate) fn actor(b: &Value) -> Option<String> {
    text(b, "actor")
}

async fn signed_out(k: &Kernel, actor: &str, person: &str, ended: Vec<String>) -> Response {
    let n = ended.len();
    let evs: Vec<(&str, Ev)> = ended.into_iter().map(|session| (actor, Ev::SignedOut { person: person.to_string(), session })).collect();
    if !evs.is_empty() {
        if let Err(e) = k.emit_all(evs).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response();
        }
    }
    Json(json!({ "ended": n })).into_response()
}

/// End every session a person holds here, as `actor`.
async fn end_sessions(State(k): State<Arc<Kernel>>, axum::extract::Path(sub): axum::extract::Path<String>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return (StatusCode::BAD_REQUEST, Json(json!({ "error": "name who ends them" }))).into_response() };
    let Some(person) = person_of(&k, &sub) else { return Json(json!({ "ended": 0 })).into_response() };
    match k.accounts.end_all(&person).await {
        Ok(ended) => signed_out(&k, &actor, &person, ended).await,
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response(),
    }
}

/// End one of them.
async fn end_session(State(k): State<Arc<Kernel>>, axum::extract::Path((sub, id)): axum::extract::Path<(String, String)>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return (StatusCode::BAD_REQUEST, Json(json!({ "error": "name who ends it" }))).into_response() };
    let gone = match person_of(&k, &sub) {
        Some(person) => k.accounts.end(&person, &id).await.map(|s| s.map(|s| (person, s))),
        None => Ok(None),
    };
    match gone {
        Ok(Some((person, s))) => signed_out(&k, &actor, &person, vec![s.id]).await,
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({ "error": "no such session" }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response(),
    }
}

// The organisation's apps and their credentials (admin console spec,
// Security › Connectors and credentials), as `actor`: the kernel's own
// operations, the same the app's owner routes use. Credentials go in and
// never come out.

pub(crate) fn done(r: Result<Value, crate::connectors::Refusal>) -> Response {
    match r {
        Ok(v) => Json(v).into_response(),
        Err((st, m)) => (st, Json(json!({ "error": m }))).into_response(),
    }
}

pub(crate) fn no_actor() -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": "name who does this" }))).into_response()
}

async fn connectors(State(k): State<Arc<Kernel>>) -> Response {
    Json(json!({ "connectors": k.admin_connectors() })).into_response()
}

async fn add_connector(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return no_actor() };
    done(k.add_connector(&actor, &b).await)
}

async fn connector_settings(State(k): State<Arc<Kernel>>, axum::extract::Path(name): axum::extract::Path<String>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return no_actor() };
    let list = |f: &str| b.get(f).and_then(|g| g.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect());
    done(k.connector_settings(&actor, &name, b["enabled"].as_bool(), list("access_groups"), list("blocked_groups")).await)
}

async fn remove_connector(State(k): State<Arc<Kernel>>, axum::extract::Path(name): axum::extract::Path<String>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return no_actor() };
    done(k.remove_connector(&actor, &name).await)
}

// One person, as root acts on them (admin console spec, Users): their
// schedules suspended and resumed, their data read, their data deleted. The
// console names people by their provider `sub`; one never seen here has
// nothing to show and nothing to delete.

fn person_refused((st, m): crate::person_admin::Refusal) -> Response {
    (st, Json(json!({ "error": m }))).into_response()
}

async fn person_schedules(State(k): State<Arc<Kernel>>, axum::extract::Path(sub): axum::extract::Path<String>) -> Response {
    let Some(person) = person_of(&k, &sub) else { return Json(json!({ "schedules": [] })).into_response() };
    match k.person_schedules(&person).await {
        Ok(v) => Json(json!({ "schedules": v })).into_response(),
        Err(r) => person_refused(r),
    }
}

async fn person_schedule(State(k): State<Arc<Kernel>>, axum::extract::Path((sub, id, change)): axum::extract::Path<(String, String, String)>, Json(b): Json<Value>) -> Response {
    let (Some(actor), Some(person)) = (actor(&b), person_of(&k, &sub)) else { return person_refused((StatusCode::NOT_FOUND, "no such schedule".into())) };
    let on = match change.as_str() {
        "resume" => true,
        "suspend" => false,
        _ => return person_refused((StatusCode::NOT_FOUND, "suspend or resume".into())),
    };
    let by = text(&b, "name").unwrap_or_else(|| "an admin".into());
    match k.admin_schedule(&actor, &by, &person, &id, on).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => person_refused(r),
    }
}

/// A read: a POST, since it is recorded and the person is told.
async fn person_data(State(k): State<Arc<Kernel>>, axum::extract::Path((sub, what)): axum::extract::Path<(String, String)>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return person_refused((StatusCode::BAD_REQUEST, "name who reads it".into())) };
    let Some(target) = person_of(&k, &sub).and_then(|p| k.person(&p)) else {
        return person_refused((StatusCode::NOT_FOUND, "they have never signed in to Magnus: there is no data of theirs here".into()));
    };
    let by = text(&b, "name").unwrap_or_else(|| "An admin".into());
    match k.read_person_data(&actor, &by, &target, &what, b["item"].as_str()).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => person_refused(r),
    }
}

async fn erase_person(State(k): State<Arc<Kernel>>, axum::extract::Path(sub): axum::extract::Path<String>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return person_refused((StatusCode::BAD_REQUEST, "name who deletes it".into())) };
    let Some(person) = person_of(&k, &sub) else { return Json(json!({ "never_here": true })).into_response() };
    match k.erase_person(&actor, &person).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => person_refused(r),
    }
}

// The agent policy (admin console spec, Security › Agent policy), as the
// kernel keeps it: sets, drafts, versions, tests.

fn policy_refused((st, m): crate::policy_admin::Refusal) -> Response {
    (st, Json(json!({ "error": m }))).into_response()
}

/// The sets, and what a test can name: the appliance's people and the
/// organisation's tools.
async fn policy_sets(State(k): State<Arc<Kernel>>) -> Response {
    let people: Vec<Value> = k.cfg.people.iter().filter_map(|p| k.person(&p.id)).map(|p| json!({ "id": p.id, "name": p.name, "groups": p.groups })).collect();
    let tools: Vec<Value> = k
        .connectors()
        .into_iter()
        .flat_map(|c| c.tools.iter().map(|(t, cl)| json!({ "name": format!("{}__{t}", c.name), "risk": cl.risk, "audience": c.audience })).collect::<Vec<_>>())
        .collect();
    Json(json!({ "sets": k.policy_sets(), "people": people, "tools": tools })).into_response()
}

async fn policy_draft(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let (Some(actor), Some(layer)) = (actor(&b), text(&b, "layer")) else { return policy_refused((StatusCode::BAD_REQUEST, "name who drafts which layer".into())) };
    match k.policy_draft(&actor, &layer, b["cedar"].as_str().map(String::from)).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => policy_refused(r),
    }
}

async fn policy_apply(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let (Some(actor), Some(layer)) = (actor(&b), text(&b, "layer")) else { return policy_refused((StatusCode::BAD_REQUEST, "name who applies which layer".into())) };
    match k.policy_apply(&actor, &layer, b["cedar"].as_str().unwrap_or_default()).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => policy_refused(r),
    }
}

async fn policy_test(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    match k.policy_test(&b) {
        Ok(v) => Json(v).into_response(),
        Err(r) => policy_refused(r),
    }
}

async fn policy_history(State(k): State<Arc<Kernel>>, Query(q): Query<HashMap<String, String>>) -> Response {
    Json(json!({ "versions": k.policy_history(q.get("layer").map(String::as_str).unwrap_or("org")) })).into_response()
}

/// The console's own session lifetimes, as an admin last set them.
async fn console_settings(State(k): State<Arc<Kernel>>) -> Response {
    Json(k.read(|s| s.console_settings.clone()).unwrap_or(json!({}))).into_response()
}

async fn set_console_settings(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return policy_refused((StatusCode::BAD_REQUEST, "name who sets them".into())) };
    let now = k.read(|s| s.console_settings.clone()).unwrap_or(json!({}));
    let idle = b["idle_minutes"].as_i64().or(now["idle_minutes"].as_i64()).unwrap_or(30);
    let hours = b["session_hours"].as_i64().or(now["session_hours"].as_i64()).unwrap_or(12);
    let allow: Option<Vec<String>> = b
        .get("allow")
        .and_then(|a| a.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .or_else(|| serde_json::from_value(now["allow"].clone()).ok());
    match k.emit(&actor, Ev::ConsoleSettings { idle_minutes: idle, session_hours: hours, allow: allow.clone() }).await {
        Ok(_) => Json(json!({ "idle_minutes": idle, "session_hours": hours, "allow": allow })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response(),
    }
}

// Operations (admin console spec): the run modules, the model providers and
// the updates, as the kernel's own operations.

fn ops_refused((st, m): crate::ops_admin::Refusal) -> Response {
    (st, Json(json!({ "error": m }))).into_response()
}

fn ops(r: Result<Value, crate::ops_admin::Refusal>) -> Response {
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => ops_refused(e),
    }
}

async fn modules(State(k): State<Arc<Kernel>>) -> Response {
    Json(json!({ "modules": k.admin_modules() })).into_response()
}

async fn set_module(State(k): State<Arc<Kernel>>, axum::extract::Path(name): axum::extract::Path<String>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return ops_refused((StatusCode::BAD_REQUEST, "name who sets it".into())) };
    ops(k.set_module(&actor, &name, b["on"].as_bool()).await)
}

async fn providers(State(k): State<Arc<Kernel>>) -> Response {
    Json(json!({ "providers": k.admin_providers().await })).into_response()
}

async fn test_provider(State(k): State<Arc<Kernel>>, axum::extract::Path(id): axum::extract::Path<String>) -> Response {
    ops(k.test_provider(&id).await)
}

async fn set_provider(State(k): State<Arc<Kernel>>, axum::extract::Path(id): axum::extract::Path<String>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return ops_refused((StatusCode::BAD_REQUEST, "name who sets it".into())) };
    ops(k.set_provider(&actor, &id, b["enabled"].as_bool(), b["default_model"].as_str()).await)
}

async fn updates(State(k): State<Arc<Kernel>>) -> Response {
    Json(k.admin_updates()).into_response()
}

/// Install the latest release now; the switch happens after the answer.
async fn install_update(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return ops_refused((StatusCode::BAD_REQUEST, "name who installs it".into())) };
    let current = crate::update::version();
    let to = k.latest_release().and_then(|m| m["version"].as_str().map(String::from));
    match to {
        Some(to) if crate::update::newer(&to, &current) && !k.read(|s| s.skipped_releases.contains(&to)) => {
            tokio::spawn(async move {
                if let Err(e) = k.update(&actor).await {
                    tracing::error!("update: {e:#}");
                }
            });
            (StatusCode::ACCEPTED, Json(json!({ "status": "installing", "version": current, "latest": to }))).into_response()
        }
        to => Json(json!({ "status": "up_to_date", "version": current, "latest": to })).into_response(),
    }
}

/// Go back to the release kept; the switch happens after the answer.
async fn roll_back_update(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return ops_refused((StatusCode::BAD_REQUEST, "name who rolls it back".into())) };
    let Some(to) = crate::update::previous_version() else { return ops_refused((StatusCode::CONFLICT, "no earlier release is kept on this server".into())) };
    tokio::spawn(async move {
        if let Err(e) = k.roll_back(&actor).await {
            tracing::error!("roll back: {e:#}");
        }
    });
    (StatusCode::ACCEPTED, Json(json!({ "status": "rolling_back", "version": crate::update::version(), "to": to }))).into_response()
}

async fn desktop_channel(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return ops_refused((StatusCode::BAD_REQUEST, "name who sets it".into())) };
    ops(k.set_desktop_channel(&actor, b["channel"].as_str().unwrap_or_default()).await)
}

// The organisation's knowledge (admin console spec, Knowledge), as the
// kernel keeps it: the org context bundle. A change names the admin by
// their provider `sub`, and is written as `human:<sub>`.

fn know(r: Result<Value, crate::knowledge_admin::Refusal>) -> Response {
    match r {
        Ok(v) => Json(v).into_response(),
        Err((st, m)) => (st, Json(json!({ "error": m }))).into_response(),
    }
}

fn sub_of(b: &Value) -> Option<String> {
    text(b, "sub").filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
}

fn no_sub() -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": "name the admin" }))).into_response()
}

async fn knowledge_list(State(k): State<Arc<Kernel>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let kinds: Vec<&str> = q.get("kinds").map(|k| k.split(',').collect()).unwrap_or_default();
    know(k.admin_bundle(&kinds).await.map(|d| json!({ "documents": d })))
}

async fn knowledge_doc(State(k): State<Arc<Kernel>>, Query(q): Query<HashMap<String, String>>) -> Response {
    know(k.admin_bundle_doc(q.get("path").map(String::as_str).unwrap_or_default()).await)
}

async fn knowledge_version(State(k): State<Arc<Kernel>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let v = q.get("version").and_then(|v| v.parse().ok()).unwrap_or(0);
    know(k.admin_bundle_version(q.get("path").map(String::as_str).unwrap_or_default(), v).await)
}

async fn knowledge_write(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let Some(sub) = sub_of(&b) else { return no_sub() };
    know(k.admin_bundle_write(&sub, b["path"].as_str().unwrap_or_default(), &b["frontmatter"], b["body"].as_str().unwrap_or_default()).await.map(|v| json!({ "version": v })))
}

async fn knowledge_remove(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let Some(sub) = sub_of(&b) else { return no_sub() };
    know(k.admin_bundle_remove(&sub, b["path"].as_str().unwrap_or_default()).await)
}

async fn knowledge_rollback(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let Some(sub) = sub_of(&b) else { return no_sub() };
    know(k.admin_bundle_rollback(&sub, b["path"].as_str().unwrap_or_default(), b["version"].as_u64().unwrap_or(0) as u32).await.map(|v| json!({ "version": v })))
}

async fn knowledge_merge(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let Some(sub) = sub_of(&b) else { return no_sub() };
    know(k.admin_merge_types(&sub, b["from"].as_str().unwrap_or_default(), b["into"].as_str().unwrap_or_default()).await)
}

async fn knowledge_activity(State(k): State<Arc<Kernel>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let limit = q.get("limit").and_then(|l| l.parse().ok()).unwrap_or(500);
    know(k.admin_activity(q.get("agents").is_some_and(|a| a == "1"), limit).await.map(|c| json!({ "changes": c })))
}

async fn knowledge_change(State(k): State<Arc<Kernel>>, axum::extract::Path(seq): axum::extract::Path<i64>) -> Response {
    know(k.admin_change(seq).await)
}

async fn knowledge_revert(State(k): State<Arc<Kernel>>, Json(b): Json<Value>) -> Response {
    let Some(sub) = sub_of(&b) else { return no_sub() };
    let seqs: Vec<i64> = b["seqs"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect();
    know(k.admin_revert(&sub, &seqs).await)
}

async fn knowledge_assembled(State(k): State<Arc<Kernel>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let person = q.get("person").cloned().or_else(|| k.cfg.people.first().map(|p| p.id.clone())).unwrap_or_default();
    know(k.admin_assembled(&person).await)
}
