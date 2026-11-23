//! The kernel's control socket: a local Unix socket in the server's state,
//! readable and writable by the server's own user alone, that `magnus kill`
//! uses when the app or sign-in is broken. Nothing reaches it over the
//! network. The app's Admin section calls the same routes in-process
//! (`admin_app::mount`), as the signed-in admin, so an admin change has one
//! implementation and one audit entry (spec rev 141, Administration). The
//! console these routes were built for is retired.

mod ops;
mod people;

use crate::events::Ev;
use crate::extract::Body;
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
    Ok(axum::serve(l, routes().with_state(k)).await?)
}

/// Every control route: the socket's, and the app's Admin section's in-process (`admin_app::mount`).
pub(crate) fn routes() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/audit", post(record).get(list))
        .route("/v1/kill-switch", post(switch).get(switch_state))
        .route("/v1/overview", axum::routing::get(overview))
        .route("/v1/people", axum::routing::get(people))
        .route("/v1/people/{person}/sessions", axum::routing::get(sessions).delete(end_sessions))
        .route("/v1/people/{person}/sessions/{id}", axum::routing::delete(end_session))
        .route("/v1/admin/people", axum::routing::get(admin_people).post(admin_invite))
        .route("/v1/admin/people/{id}", axum::routing::get(admin_person).patch(people::patch).delete(people::delete))
        .route("/v1/admin/people/{id}/disable", post(people::disable))
        .route("/v1/admin/people/{id}/enable", post(people::enable))
        .route("/v1/admin/people/{id}/cancel-invite", post(people::cancel_invite))
        .route("/v1/admin/people/{id}/invite", post(people::resend_invite))
        .route("/v1/admin/people/{id}/invite/password", post(people::invite_password))
        .route("/v1/admin/sign-in-policy", axum::routing::get(sign_in_policy).put(set_sign_in_policy))
        .route("/v1/people/{person}/schedules", axum::routing::get(person_schedules))
        .route("/v1/people/{person}/schedules/{id}/{change}", post(person_schedule))
        .route("/v1/people/{person}/erase", post(erase_person))
        .route("/v1/people/{person}/computer", post(crate::computer::make_for_person))
        .route("/v1/admin/policy", axum::routing::get(ops::policy_sets))
        .route("/v1/admin/policy/draft", post(ops::policy_draft))
        .route("/v1/admin/policy/apply", post(ops::policy_apply))
        .route("/v1/admin/policy/test", post(ops::policy_test))
        .route("/v1/admin/policy/history", axum::routing::get(ops::policy_history))
        .route("/v1/admin/modules", axum::routing::get(ops::modules))
        .route("/v1/admin/modules/{name}", post(ops::set_module))
        .route("/v1/admin/providers", axum::routing::get(ops::providers))
        .route("/v1/admin/providers/{id}", axum::routing::patch(ops::set_provider))
        .route("/v1/admin/providers/{id}/test", post(ops::test_provider))
        .route("/v1/admin/updates", axum::routing::get(ops::updates))
        .route("/v1/admin/updates/install", post(ops::install_update))
        .route("/v1/admin/updates/rollback", post(ops::roll_back_update))
        .route("/v1/admin/updates/desktop-channel", axum::routing::put(ops::desktop_channel))
        .route("/v1/admin/connectors", axum::routing::get(connectors).post(add_connector))
        .route("/v1/admin/connectors/{name}", axum::routing::patch(connector_settings).delete(remove_connector))
        .merge(crate::registrations::routes())
}

pub(super) fn text(b: &Value, f: &str) -> Option<String> {
    b[f].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// Record a console action; its seq once it is in the log.
async fn record(State(k): State<Arc<Kernel>>, Body(b): Body<Value>) -> Response {
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
        s.org
            .audit()
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
    let stopped: Vec<String> = k.read(|s| crate::halts::halted_runs(s).map(|r| r.id.clone()).collect());
    let frozen = stopped.iter().filter(|r| k.sandboxes.kept(r)).count();
    let running = k.sandboxes.running().await;
    Json(k.read(|s| {
        let halted = crate::halts::halted_runs(s).count();
        let held = if s.vault.kill_switch() { s.arrangements.schedules().values().filter(|x| x.next_due().is_some()).count() } else { 0 };
        json!({
            "engaged": s.vault.kill_switch(),
            "by": s.vault.kill_switch_by().map(|(who, _)| who), "at": s.vault.kill_switch_by().map(|(_, at)| at),
            "tasks_halted": halted, "schedules_held": held, "sign_ins_revoked": s.vault.revoked().len(),
            "microvms_frozen": frozen, "microvms_running": running,
        })
    }))
    .into_response()
}

/// Engage or release it, as `actor`: the one switch, the app's own.
async fn switch(State(k): State<Arc<Kernel>>, Body(b): Body<Value>) -> Response {
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
        failed.sort_by_key(|a| std::cmp::Reverse(a.ended_at));
        let knowledge = s.library.knowledge().values().filter(|e| e.at > day && !e.author.starts_with("person:")).count();
        let memory = s.library.memory().iter().filter(|m| m.at > day).count();
        // Agents' changes to the org context bundle (admin console spec,
        // Overview: "agent knowledge changes in the last 24 hours").
        let bundle = s.library.bundle().values().filter(|d| d.scope == "org").flat_map(|d| d.versions.iter()).filter(|v| v.2 > day && !v.3.starts_with("human:")).count();
        let last_change = s.library.bundle().values().filter(|d| d.scope == "org").flat_map(|d| d.versions.iter()).map(|v| v.1).max();
        json!({
            "version": crate::update::version(), "log_up": log_up, "kill_switch": s.vault.kill_switch(),
            "running_tasks": s.runs.values().filter(|r| r.status == Status::Running).count(),
            "waiting_tasks": s.runs.values().filter(|r| r.status == Status::Suspended).count(),
            "signed_in": signed_in, "microvms_running": vms,
            "knowledge_changes_24h": knowledge + memory + bundle, "knowledge_last_change": last_change,
            "recent_errors": failed.iter().take(10).map(|r| json!({ "run": r.id, "task": r.task, "person": r.person, "reason": r.reason, "at": r.ended_at })).collect::<Vec<_>>(),
        })
    });
    Json(v).into_response()
}

/// A console path id to a person: a provider `sub` first, while the
/// provider still binds people, else the id itself when it is a person here
/// (#508: the console now addresses local ids).
fn person_of(k: &Kernel, sub: &str) -> Option<String> {
    k.read(|s| s.identity.subjects().iter().find(|(_, v)| v.as_str() == sub).map(|(p, _)| p.clone())).or_else(|| k.person(sub).map(|p| p.id.clone()))
}

/// Each person known here, by their provider `sub`: when they last signed
/// in, and how many sessions they hold open (the console's Users list; the
/// people themselves are the identity provider's).
async fn people(State(k): State<Arc<Kernel>>) -> Response {
    let bound: Vec<(String, String, Option<chrono::DateTime<chrono::Utc>>)> =
        k.read(|s| s.identity.subjects().iter().map(|(p, sub)| (p.clone(), sub.clone(), s.identity.last_sign_in(p))).collect());
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
    let mut rows = Vec::new();
    for id in k.people().ids() {
        rows.push(admin_person_row(&k, &id).await);
    }
    Json(json!({ "people": rows })).into_response()
}

async fn admin_person_row(k: &Kernel, id: &str) -> Value {
    let p = k.person(id);
    let email = k.accounts.local_person(id).and_then(|l| l.email);
    let (role, status) = tokio::join!(k.accounts.role(id), k.accounts.status(id));
    let at = k.read(|s| s.identity.last_sign_in(id));
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

/// Invite someone: the appliance makes their local account and a
/// single-use token, shown once here for the admin to pass on — the
/// appliance cannot send mail, so there is no email step (#508). The id
/// derives from the email's local part; a held id refuses `taken`,
/// never a surprising sibling.
async fn admin_invite(State(k): State<Arc<Kernel>>, Body(b): Body<Value>) -> Response {
    let email = b["email"].as_str().map(str::trim).unwrap_or_default().to_lowercase();
    let name = b["name"].as_str().map(str::trim).unwrap_or_default();
    let role = match b["role"].as_str() {
        Some("admin") => crate::accounts::Role::Admin,
        Some("member") => crate::accounts::Role::Member,
        _ => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid", "message": "The role is admin or member." }))).into_response(),
    };
    if !crate::invitations::valid_email(&email) || name.is_empty() || name.len() > 200 {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid", "message": "An invitation needs an email address and a name." }))).into_response();
    }
    let Some(id) = crate::invitations::derive_id(&email) else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid", "message": "No person id derives from that address." }))).into_response();
    };
    if k.person(&id).is_some() {
        return (StatusCode::CONFLICT, Json(json!({ "error": "taken", "message": format!("{id} is already a person here.") }))).into_response();
    }
    // An erased person's id goes to nobody new (`free_id`).
    let id = crate::invitations::free_id(&k, &email).unwrap_or(id);
    match k.invite_account(&id, name, &email, role).await {
        Ok(token) => (
            StatusCode::CREATED,
            Json(json!({ "id": id, "email": email, "invite_token": token, "invite_expires": chrono::Utc::now() + chrono::Duration::days(crate::accounts::INVITE_DAYS) })),
        )
            .into_response(),
        Err((code, "taken", message)) => (code, Json(json!({ "error": "taken", "message": message }))).into_response(),
        Err((code, _, message)) => (code, Json(json!({ "error": message }))).into_response(),
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
async fn set_sign_in_policy(State(k): State<Arc<Kernel>>, Body(b): Body<Value>) -> Response {
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
async fn end_sessions(State(k): State<Arc<Kernel>>, axum::extract::Path(sub): axum::extract::Path<String>, Body(b): Body<Value>) -> Response {
    let Some(actor) = actor(&b) else { return (StatusCode::BAD_REQUEST, Json(json!({ "error": "name who ends them" }))).into_response() };
    let Some(person) = person_of(&k, &sub) else { return Json(json!({ "ended": 0 })).into_response() };
    match k.accounts.end_all(&person).await {
        Ok(ended) => signed_out(&k, &actor, &person, ended).await,
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response(),
    }
}

/// End one of them.
async fn end_session(State(k): State<Arc<Kernel>>, axum::extract::Path((sub, id)): axum::extract::Path<(String, String)>, Body(b): Body<Value>) -> Response {
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

async fn add_connector(State(k): State<Arc<Kernel>>, Body(b): Body<Value>) -> Response {
    let Some(actor) = actor(&b) else { return no_actor() };
    done(async { k.add_connector(&actor, &serde_json::from_value(b).map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?).await }.await)
}

async fn connector_settings(State(k): State<Arc<Kernel>>, axum::extract::Path(name): axum::extract::Path<String>, Body(b): Body<Value>) -> Response {
    let Some(actor) = actor(&b) else { return no_actor() };
    let list = |f: &str| b.get(f).and_then(|g| g.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect());
    done(k.connector_settings(&actor, &name, b["enabled"].as_bool(), list("access_groups"), list("blocked_groups")).await)
}

async fn remove_connector(State(k): State<Arc<Kernel>>, axum::extract::Path(name): axum::extract::Path<String>, Body(b): Body<Value>) -> Response {
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

async fn person_schedule(State(k): State<Arc<Kernel>>, axum::extract::Path((sub, id, change)): axum::extract::Path<(String, String, String)>, Body(b): Body<Value>) -> Response {
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

async fn erase_person(State(k): State<Arc<Kernel>>, axum::extract::Path(sub): axum::extract::Path<String>, Body(b): Body<Value>) -> Response {
    let Some(actor) = actor(&b) else { return person_refused((StatusCode::BAD_REQUEST, "name who deletes it".into())) };
    let Some(person) = person_of(&k, &sub) else { return Json(json!({ "never_here": true })).into_response() };
    match k.erase_person(&actor, &person).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => person_refused(r),
    }
}
