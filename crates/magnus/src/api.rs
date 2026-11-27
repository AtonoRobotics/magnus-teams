//! The HTTP API, for people's apps. Every request names a person: by a
//! session from sign-in (the desktop app sends it as a bearer token,
//! a browser as an HttpOnly cookie) or by a configured API token. Approvals,
//! answers and policy arrive only here, a channel the agent cannot reach (its
//! sandbox has no network and no tool calls this API).

pub(crate) mod events_sse;
mod owner;
mod schedules;
mod sign_in;
mod work;
#[cfg(test)]
mod work_list_tests;

use crate::api_error::{refuse, session_unread, ApiResult};
use crate::config::Person;
use crate::events::Ev;
use crate::extract::{Body, Caller};
use crate::kernel::{new_id, Kernel, HARNESS_VERSION};
use crate::{api_bodies as req, people::rights};
use axum::{
    extract::{ConnectInfo, Path, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Extension, Json, Router,
};
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::Arc};

type K = State<Arc<Kernel>>;

pub(crate) use crate::api_error::{err, internal};
pub(crate) use sign_in::{issue, on_this_machine, over_tls};
pub(crate) use work::{chosen_model, chosen_reasoning, end_for, status_str, task_row, terms, timeline_of, title_of};

/// New work is refused while the kill switch is on, before anything about
/// it is recorded (owner's ruling 11).
pub(crate) fn refused_by_kill_switch(k: &crate::kernel::Kernel) -> Option<Response> {
    k.read(|s| s.vault.kill_switch()).then(|| internal(crate::kernel::KillSwitchOn.into()))
}

/// Which listener a request came in on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Plain,
    Tls,
}

pub const SESSION_COOKIE: &str = "magnus_session";

fn bearer(h: &HeaderMap) -> Option<&str> {
    h.get("authorization").and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "))
}

fn cookie<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    h.get_all("cookie").iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';')).filter_map(|c| c.trim().split_once('=')).find(|(n, _)| *n == name).map(|(_, v)| v)
}

pub(crate) fn credential(h: &HeaderMap) -> Option<&str> {
    bearer(h).or_else(|| cookie(h, SESSION_COOKIE))
}

pub(crate) async fn who(k: &Kernel, h: &HeaderMap) -> Result<Person, Box<Response>> {
    let token = credential(h).ok_or_else(|| Box::new(err(StatusCode::UNAUTHORIZED, "sign in")))?;
    if let Some(p) = k.cfg.person_by_token(token) {
        return k.person(&p.id).ok_or_else(|| Box::new(err(StatusCode::UNAUTHORIZED, "sign in again")));
    }
    k.accounts
        .session(token)
        .await
        .map_err(|e| Box::new(session_unread(e)))?
        .and_then(|s| k.person(&s.person))
        .ok_or_else(|| Box::new(err(StatusCode::UNAUTHORIZED, "sign in again")))
}

/// A browser sends the session cookie on its own, so a change made with the
/// cookie must also carry a header another site's page cannot set. The
/// recovery, invitation and join forms are the exception: each is authorized
/// by its token or code, never the cookie, so a stale session cookie must
/// not block it (#541).
async fn cookie_guard(req: Request, next: Next) -> Response {
    let h = req.headers();
    let safe = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    let recovery = *req.method() == Method::POST && matches!(req.uri().path(), "/reset" | "/invite" | "/join");
    if !safe && !recovery && bearer(h).is_none() && cookie(h, SESSION_COOKIE).is_some() && h.get("x-magnus-client").is_none() {
        return err(StatusCode::FORBIDDEN, "missing x-magnus-client header");
    }
    // Those forms sign the browser in, so another site's page must not be
    // able to submit one and sign its visitor in to an account it chose.
    if recovery && !same_origin(h) {
        return err(StatusCode::FORBIDDEN, "this form is sent from its own page");
    }
    next.run(req).await
}

/// Whether a browser sent this request from one of the server's own pages:
/// `Sec-Fetch-Site` when the browser says, else an `Origin` naming this
/// host. A client that sends neither is no browser and is let through.
fn same_origin(h: &HeaderMap) -> bool {
    if let Some(site) = h.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        return matches!(site, "same-origin" | "none");
    }
    let Some(origin) = h.get("origin").and_then(|v| v.to_str().ok()) else { return true };
    let host = h.get("host").and_then(|v| v.to_str().ok());
    url::Url::parse(origin).ok().is_some_and(|o| {
        let authority = match (o.host_str(), o.port()) {
            (Some(name), Some(port)) => format!("{name}:{port}"),
            (Some(name), None) => name.to_string(),
            _ => return false,
        };
        host.is_some_and(|h| h.eq_ignore_ascii_case(&authority))
    })
}

pub fn router(k: Arc<Kernel>, transport: Transport) -> Router {
    use {owner::*, schedules::*, sign_in::*, work::*};
    Router::new()
        .route("/", get(crate::ui::assets::ui_index))
        .route(crate::connectors::web::RETURN_PATH, get(crate::connectors::web::connected))
        .route("/{file}", get(crate::ui::assets::ui_file))
        .route("/fonts/{file}", get(crate::ui::assets::ui_font))
        .route("/v1/health", get(health))
        .route("/v1/events", get(events_sse::events_stream))
        .route("/v1/sign-in", get(sign_in_method).post(sign_in))
        .route("/v1/devices", get(list_devices).post(enroll_device))
        .route("/v1/devices/{id}", axum::routing::delete(revoke_device))
        .route("/v1/sign-in/browser", post(browser_begin))
        .route("/v1/sign-in/browser/finish", post(browser_finish))
        .route("/v1/sign-in/device", post(device_start))
        .route("/v1/sign-in/device/{flow}", post(device_poll))
        .route("/v1/invites/accept", post(accept_invite))
        .route("/reset", get(reset_page).post(reset_confirm))
        .route("/v1/password", post(choose_password))
        .route("/v1/sign-out", post(sign_out))
        .route("/v1/tasks/{id}/live", get(live))
        .route("/v1/tasks/{id}/outputs/{out}", get(output))
        .route("/v1/transcribe", post(transcribe))
        .route("/v1/speak", post(speak))
        .route("/v1/me", get(me))
        .route("/v1/versions", get(versions))
        .route("/v1/tasks", post(submit).get(list_tasks))
        .route("/v1/tasks/stop-all", post(stop_all))
        .route("/v1/chat", post(chat_post).get(chat_threads))
        .route("/v1/chat/{thread}", get(chat_thread))
        .route("/v1/chat/cards/{card}", post(chat_card))
        .route("/v1/tasks/{id}", get(task))
        .route("/v1/tasks/{id}/timeline", get(timeline))
        .route("/v1/tasks/{id}/resume", post(resume))
        .route("/v1/tasks/{id}/pause", post(pause_task))
        .route("/v1/tasks/{id}/cancel", post(cancel))
        .route("/v1/tasks/{id}/steer", post(steer))
        .route("/v1/tasks/{id}/verdict", post(verdict))
        .route("/v1/tasks/{id}/undo/{call_id}", post(undo_task_action))
        .route("/v1/briefing", get(briefing))
        .route("/v1/search", get(search))
        .route("/v1/inbox", get(inbox_list))
        .route("/v1/inbox/{id}", post(respond))
        .route("/v1/inbox/opened", post(inbox_opened))
        .route("/v1/inbox/batch", post(inbox_batch))
        .route("/v1/staged", get(crate::staging::list_route))
        .route("/v1/staged/{id}", post(crate::staging::resolve_route))
        .route("/v1/explore/scan", post(explore_scan))
        .route("/v1/home/opened", post(home_opened))
        .route("/v1/policies", get(policies))
        .route("/v1/policies/{*layer}", put(put_policy))
        .route("/v1/secrets", get(secret_names))
        .route("/v1/secrets/{name}", put(put_secret))
        .route("/v1/kill-switch", post(kill_switch).get(kill_switch_state))
        .route("/v1/backups", get(backups).post(back_up))
        .route("/v1/adapters", get(adapters))
        .route("/v1/model", get(get_model).put(put_model))
        .route("/v1/models", get(models))
        .route("/v1/adapters/{id}", put(put_adapter))
        .route("/v1/adapters/{id}/catalog", get(catalog))
        .route("/v1/adapters/{id}/conformance", post(conformance))
        .route("/v1/adapters/{id}/sign-in", post(sign_in_begin))
        .route("/v1/adapters/{id}/sign-in/complete", post(sign_in_complete))
        .route("/v1/memory", get(memory).post(add_memory))
        .route("/v1/memory/summary", get(memory_summary))
        .route("/v1/memory/{id}", axum::routing::delete(forget))
        .route("/v1/playbook", get(playbook_list))
        .route("/v1/playbook/{id}", axum::routing::delete(playbook_retire))
        .route("/v1/schedules", get(schedules).post(add_schedule))
        .route("/v1/schedules/{id}", axum::routing::delete(remove_schedule).patch(edit_schedule))
        .route("/v1/schedules/{id}/resume", post(resume_schedule))
        .route("/v1/schedules/{id}/pause", post(pause_schedule))
        .route("/v1/schedules/{id}/run", post(run_schedule_now))
        .route("/v1/schedules/{id}/stop", post(stop_schedule))
        .merge(crate::dev::router().merge(crate::legal::routes::router(k.clone())))
        .merge(crate::connectors::router().merge(crate::generated::router()).merge(crate::payments::router()).merge(crate::push::router()))
        .merge(crate::library::router().merge(crate::consent::router()))
        .merge(crate::update::router())
        .merge(crate::settings::router().merge(crate::passwords::router()))
        .merge(crate::relationship::router())
        .merge(crate::reactions::router().merge(crate::a2a::router()))
        .merge(crate::grants::router().merge(crate::computer::router()).merge(crate::rules::router()))
        .merge(crate::commitments::router())
        .merge(crate::knowledge::router().merge(crate::chat_actions::router()).merge(crate::spend::routes::router()))
        .merge(crate::trash::router().merge(crate::admin_app::router(k.clone())))
        .layer(axum::middleware::from_fn_with_state(k.clone(), crate::admin_app::settings_gate::gate)) // an admin change outside Admin: step-up and audit
        .layer(axum::middleware::from_fn(cookie_guard))
        .layer(axum::middleware::from_fn(crate::update::app_version))
        .layer(axum::middleware::from_fn_with_state(k.clone(), config_tokens_on_this_machine))
        .layer(Extension(transport))
        .layer(axum::middleware::from_fn_with_state(k.clone(), require_device_cert))
        .with_state(k)
}

/// A tunnel request, once device certificates are configured, must present
/// a paired device, and one its session may be used from: the session
/// person's own, and the session's own device when it has one
/// (`devices::session_holds`). The header is only believed from loopback
/// with `Cf-Connecting-Ip`: that is cloudflared. A client on the LAN can set
/// the same header and is not held to it.
async fn require_device_cert(State(k): State<Arc<Kernel>>, req: Request, next: Next) -> ApiResult {
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
    let tunnelish = req.headers().contains_key("cf-connecting-ip");
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let device = crate::devices::presented(req.headers()).and_then(|fp| k.devices.live_device(fp));
    let on = k.cfg.client_certs.is_some();
    let decision = match peer {
        Some(p) => crate::devices::admit(on, p, req.headers(), &method, &path, device.is_some()),
        None if on && tunnelish => Err("this device is not paired with the server"),
        None => Ok(()),
    };
    if let Err(msg) = decision {
        return refuse(StatusCode::UNAUTHORIZED, msg);
    }
    let tunnel = on && peer.is_some_and(|p| crate::devices::is_tunnel(p, req.headers()));
    if let (true, Some(d), Some(token)) = (tunnel, &device, credential(req.headers()).map(str::to_string)) {
        let holder = match k.cfg.person_by_token(&token) {
            Some(p) => Some((p.id.clone(), None)),
            None => match k.accounts.session(&token).await {
                Ok(s) => s.map(|s| (s.person, s.device)),
                Err(e) => return Ok(session_unread(e)),
            },
        };
        if let Some((person, bound)) = holder {
            if let Err(msg) = crate::devices::session_holds(d, &person, bound.as_deref()) {
                return refuse(StatusCode::UNAUTHORIZED, msg);
            }
        }
    }
    Ok(next.run(req).await)
}

/// The config's long-lived tokens (the owner's scripts, the evaluation
/// runner) work only on the plain listener, from this machine: nowhere a
/// stolen copy could be used from the network (owner's ruling 13).
async fn config_tokens_on_this_machine(State(k): State<Arc<Kernel>>, req: Request, next: Next) -> ApiResult {
    if credential(req.headers()).is_some_and(|t| k.cfg.person_by_token(t).is_some()) {
        let plain = req.extensions().get::<Transport>() == Some(&Transport::Plain);
        let local = req.extensions().get::<axum::extract::ConnectInfo<SocketAddr>>().is_some_and(|c| c.0.ip().is_loopback());
        if !(plain && local) {
            return refuse(StatusCode::UNAUTHORIZED, "This key works only on the server itself, over its local address. Sign in to use Magnus from elsewhere.");
        }
    }
    Ok(next.run(req).await)
}

async fn me(State(k): K, h: HeaderMap) -> ApiResult {
    Ok(match who(&k, &h).await {
        Ok(p) => Json(json!({ "id": p.id, "name": p.name, "groups": p.groups, "roles": p.roles, "voice": k.cfg.speech.is_some(), "read_aloud": k.cfg.read_aloud.is_some() }))
            .into_response(),
        Err(r) => *r,
    })
}

/// What the appliance is running now, and every change to it, so apps can
/// tell people when the agent they work with has changed (U5).
async fn versions(State(k): K, _: Caller) -> ApiResult {
    let v = k.read(|s| {
        let changes: Vec<Value> = s.org.version_changes().iter().map(|(seq, at, what, value)| json!({ "seq": seq, "at": at, "what": what, "value": value })).collect();
        json!({
            "harness": crate::kernel::HARNESS_VERSION,
            "instructions": crate::run::instructions_version(),
            "chat_instructions": crate::run::chat_instructions_version(),
            "model": s.org.model().as_ref().map(|(a, m)| format!("{a}/{m}")),
            "playbook": s.library.playbook_version(),
            "latest_seq": s.org.version_changes().last().map(|c| c.0),
            "changes": changes,
        })
    });
    Ok(Json(v).into_response())
}

/// A control's answer as the app gets it.
fn controlled(r: Result<Value, crate::controls::Refused>) -> Response {
    use crate::controls::Refused;
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e @ Refused::NotFound(_)) => err(StatusCode::NOT_FOUND, e.to_string()),
        Err(Refused::Denied(m)) => err(StatusCode::FORBIDDEN, m),
        Err(Refused::Conflict(m)) => err(StatusCode::CONFLICT, m),
        Err(Refused::Invalid(m)) => err(StatusCode::BAD_REQUEST, m),
        Err(Refused::Failed(m)) => err(StatusCode::INTERNAL_SERVER_ERROR, m),
    }
}

async fn memory(State(k): K, Caller(p): Caller) -> ApiResult {
    // Recalled for the person, through their wall.
    let items: Vec<Value> = match k.remembered(&p.id, |_| true).await {
        Ok(r) => r
            .into_iter()
            .map(|r| {
                let m = r.entry;
                json!({ "id": m.id, "fact": r.fact, "topic": r.topic, "label": m.label, "run": m.run, "by": m.by.as_deref().unwrap_or("model"), "at": m.at })
            })
            .collect(),
        Err(e) => return Ok(internal(e)),
    };
    Ok(Json(json!({ "memory": items })).into_response())
}

async fn add_memory(State(k): K, Caller(p): Caller, Body(b): Body<Value>) -> ApiResult {
    let fact = b["fact"].as_str().unwrap_or_default().trim();
    if fact.is_empty() || fact.chars().count() > crate::compose::MAX_TEXT {
        return refuse(StatusCode::BAD_REQUEST, "memory fact is required");
    }
    let topic = b["topic"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let id = new_id("mem");
    let actor = format!("person:{}", p.id);
    k.emit(
        &actor,
        Ev::Memory(crate::events::MemoryPayload {
            id: id.clone(),
            person: p.id.clone(),
            run: String::new(),
            fact: fact.to_string(),
            topic,
            label: crate::tools::Label::PRINCIPAL,
            sources: vec![],
            by: Some("person".into()),
        }),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))).into_response())
}

async fn memory_summary(State(k): K, Caller(p): Caller) -> ApiResult {
    Ok(Json(k.memory_summary_json(&p.id).await).into_response())
}

/// The person's active playbook lessons (user-facing, not gated behind Developer).
async fn playbook_list(State(k): K, Caller(p): Caller) -> ApiResult {
    let items: Vec<Value> = k.read(|s| {
        let ver = s.library.playbook_version();
        s.playbook_at(&p.id, ver)
            .into_iter()
            .map(|item| json!({ "id": item.id, "text": item.text, "addresses": item.addresses, "label": item.label, "added": item.added }))
            .collect()
    });
    Ok(Json(json!({ "playbook": items })).into_response())
}

/// Retire one of the person's own playbook lessons.
async fn playbook_retire(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    let exists = k.read(|s| {
        let ver = s.library.playbook_version();
        s.playbook_at(&p.id, ver).into_iter().any(|item| item.id == id)
    });
    if !exists {
        return refuse(StatusCode::NOT_FOUND, "no such playbook lesson");
    }
    let actor = format!("person:{}", p.id);
    k.emit(
        &actor,
        Ev::Playbook(crate::events::PlaybookPayload {
            item: id,
            op: "retire".into(),
            person: p.id.clone(),
            run: String::new(),
            text: None,
            addresses: None,
            label: crate::tools::Label::PRINCIPAL,
        }),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Maximum inbox items resolved in one batch call.
const MAX_BATCH_INBOX_ITEMS: usize = 50;

/// Resolve multiple inbox items at once (e.g. approve/dismiss selected).
async fn inbox_batch(State(k): K, Caller(p): Caller, Body(b): Body<Value>) -> ApiResult {
    let Some(ids) = b["ids"].as_array() else {
        return refuse(StatusCode::BAD_REQUEST, "ids array is required");
    };
    if ids.is_empty() || ids.len() > MAX_BATCH_INBOX_ITEMS {
        return refuse(StatusCode::BAD_REQUEST, format!("provide between 1 and {MAX_BATCH_INBOX_ITEMS} ids"));
    }
    let decision = b["decision"].as_str().unwrap_or_default();
    if decision.is_empty() {
        return refuse(StatusCode::BAD_REQUEST, "decision is required");
    }
    let item_body = json!({
        "decision": decision,
        "scope": b.get("scope").cloned().unwrap_or(Value::Null),
        "text": b.get("text").cloned().unwrap_or(Value::Null),
    });
    let mut results = Vec::with_capacity(ids.len());
    for raw in ids {
        let Some(id) = raw.as_str() else { continue };
        match k.respond(&p, id, &item_body).await {
            Ok(crate::inbox::Answer::Ok) => results.push(json!({ "id": id, "ok": true })),
            Ok(crate::inbox::Answer::Forbidden(m)) | Ok(crate::inbox::Answer::Conflict(m)) => results.push(json!({ "id": id, "ok": false, "error": m })),
            Ok(crate::inbox::Answer::NotFound) => results.push(json!({ "id": id, "ok": false, "error": "not found" })),
            Ok(crate::inbox::Answer::Invalid(m)) => results.push(json!({ "id": id, "ok": false, "error": m })),
            Err(e) => results.push(json!({ "id": id, "ok": false, "error": e.to_string() })),
        }
    }
    Ok(Json(json!({ "results": results })).into_response())
}

/// Fire a schedule immediately on demand ("Run now"), or preview it when `{ "dry_run": true }`.
async fn run_schedule_now(State(k): K, Caller(p): Caller, Path(id): Path<String>, body: Option<Json<Value>>) -> ApiResult {
    let Some(sc) = k.read(|s| s.arrangements.schedules().get(&id).cloned()) else {
        return refuse(StatusCode::NOT_FOUND, "no such schedule");
    };
    if sc.person != p.id {
        return refuse(StatusCode::FORBIDDEN, "only its owner may run a schedule");
    }
    if sc.ended.is_some() {
        return refuse(StatusCode::CONFLICT, "this schedule has ended");
    }
    let dry_run = body.as_ref().is_some_and(|Json(v)| v["dry_run"].as_bool().unwrap_or(false));
    if dry_run {
        let words = k.words(vec![(sc.person.clone(), sc.seq)]).await.ok();
        let prompt = words.as_ref().map(|w| w.text(sc.seq).to_string()).unwrap_or_default();
        return Ok(Json(json!({
            "dry_run": true,
            "schedule_id": sc.id,
            "every_minutes": sc.every_minutes,
            "daily_at": sc.daily_at,
            "next_due": sc.next_due(),
            "paused": sc.paused,
            "prompt_preview": prompt,
            "would_run": sc.paused.is_none() && sc.ended.is_none(),
        }))
        .into_response());
    }
    match k.fire_schedule_now(sc).await {
        Ok(task) => Ok((StatusCode::CREATED, Json(json!({ "task_id": task }))).into_response()),
        Err(e) => Ok(internal(e)),
    }
}

/// Pause a running task without cancelling it; resume later with POST /v1/tasks/{id}/resume.
async fn pause_task(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    let Some((task, run)) = k.read(|s| s.tasks.get(&id).and_then(|t| s.runs.get(&t.run_id).map(|r| (t.clone(), r.clone())))) else {
        return refuse(StatusCode::NOT_FOUND, "no such task");
    };
    if task.person != p.id {
        return refuse(StatusCode::FORBIDDEN, "only its owner may pause a task");
    }
    if run.status != crate::state::Status::Running {
        return refuse(StatusCode::CONFLICT, "only a running task can be paused");
    }
    let actor = format!("person:{}", p.id);
    k.emit(&actor, Ev::Suspended { run: run.id.clone(), reason: "paused_by_user".into(), item: None }).await?;
    Ok(Json(json!({ "ok": true, "status": "suspended" })).into_response())
}

/// Recent-window horizon for Morning Briefing completed tasks.
const BRIEFING_RECENT_HOURS: i64 = 24;

/// Synthesized, instant server-side executive briefing for the signed-in person.
async fn briefing(State(k): K, Caller(p): Caller) -> ApiResult {
    let now = chrono::Utc::now();
    let cutoff = now - chrono::Duration::hours(BRIEFING_RECENT_HOURS);
    let (needs, staged_count, active_tasks, completed_recent, upcoming, cmt_meta) = k.read(|s| {
        let needs: Vec<Value> = s
            .inbox
            .items()
            .values()
            .filter(|i| i.resolution.is_none() && crate::inbox::may_respond(s, &p, i))
            .map(|i| json!({ "id": i.id, "kind": crate::inbox::inbox_kind(&i.kind), "summary": i.summary, "blocking": crate::inbox::blocking(s, i), "created_at": i.created_at }))
            .collect();
        let staged_count = s.staged.open_for(&p.id).len();
        let mut active_tasks = Vec::new();
        let mut completed_recent = Vec::new();
        for t in s.tasks.values().filter(|t| t.person == p.id && !t.review && !t.poll) {
            let Some(r) = s.runs.get(&t.run_id) else { continue };
            if !r.status.terminal() {
                active_tasks.push(json!({ "task_id": t.task_id, "status": r.status, "waiting_item": r.waiting_item, "created_at": t.created_at }));
            } else if r.status == crate::state::Status::Completed && r.ended_at.unwrap_or(t.created_at) >= cutoff {
                completed_recent.push(json!({
                    "task_id": t.task_id,
                    "summary": r.summary.as_deref().unwrap_or_default().chars().take(180).collect::<String>(),
                    "ended_at": r.ended_at,
                }));
            }
        }
        let mut upcoming: Vec<(chrono::DateTime<chrono::Utc>, Value)> = s
            .arrangements
            .schedules()
            .values()
            .filter(|sc| sc.person == p.id && sc.ended.is_none() && sc.paused.is_none())
            .filter_map(|sc| {
                let next = sc.next_due()?;
                Some((next, json!({ "id": sc.id, "next": next, "every_minutes": sc.every_minutes, "daily_at": sc.daily_at })))
            })
            .collect();
        upcoming.sort_by_key(|(next, _)| *next);
        let upcoming: Vec<Value> = upcoming.into_iter().take(5).map(|(_, v)| v).collect();
        let zone = s.identity.zone(&p.id);
        let cmt_meta: Vec<(String, i64, Option<String>, bool)> = s
            .arrangements
            .commitments()
            .values()
            .filter(|c| c.person == p.id && c.status == "open")
            .filter_map(|c| {
                let due = c.by.as_deref().and_then(|b| crate::commitments::due(b, zone))?;
                let overdue = due < now;
                (overdue || due <= now + chrono::Duration::hours(BRIEFING_RECENT_HOURS)).then(|| (c.id.clone(), c.seq, c.by.clone(), overdue))
            })
            .collect();
        (needs, staged_count, active_tasks, completed_recent, upcoming, cmt_meta)
    });
    let c_seqs: Vec<i64> = cmt_meta.iter().map(|(_, seq, ..)| *seq).collect();
    let c_content = if c_seqs.is_empty() { std::collections::HashMap::new() } else { k.recall.content(&p.id, &c_seqs).await.unwrap_or_default() };
    let commitments: Vec<Value> = cmt_meta
        .into_iter()
        .map(|(id, seq, by, overdue)| {
            let what = c_content.get(&seq).and_then(|v| v["what"].as_str()).unwrap_or_default();
            let to = c_content.get(&seq).and_then(|v| v["to"].as_str());
            json!({ "id": id, "what": what, "to": to, "by": by, "overdue": overdue })
        })
        .collect();
    Ok(Json(json!({
        "generated_at": now,
        "needs_attention": needs,
        "staged_count": staged_count,
        "active_tasks": active_tasks,
        "completed_recent": completed_recent,
        "upcoming_schedules": upcoming,
        "overdue_commitments": commitments,
        "stats": {
            "needs_count": needs.len(),
            "staged_count": staged_count,
            "active_count": active_tasks.len(),
            "completed_24h": completed_recent.len(),
            "overdue_count": commitments.iter().filter(|c| c["overdue"] == true).count(),
        }
    }))
    .into_response())
}

/// Start a scoped undo task for a reversible action receipt in one of the person's tasks.
async fn undo_task_action(State(k): K, Caller(p): Caller, Path((id, call_id)): Path<(String, String)>) -> ApiResult {
    let found = k.read(|s| {
        let task = s.tasks.get(&id)?;
        let run = s.runs.get(&task.run_id)?;
        let intent = run.intents.values().find(|i| i.call_id == call_id)?.clone();
        Some((task.person.clone(), intent))
    });
    let Some((owner, intent)) = found else {
        return refuse(StatusCode::NOT_FOUND, "no such task action");
    };
    if owner != p.id {
        return refuse(StatusCode::FORBIDDEN, "only the task's owner may undo an action");
    }
    let reversible = crate::boundary::facts(&k, &intent.tool).is_some_and(|f| f.risk == crate::tools::Risk::ReversibleWrite);
    if !reversible {
        return refuse(StatusCode::CONFLICT, "this action is not marked as a reversible write");
    }
    let prompt = format!("Undo the reversible action `{}` (call `{}`) from task `{}`. Original arguments: {}. Confirm once reverted.", intent.tool, call_id, id, intent.input);
    let actor = format!("person:{}", p.id);
    let (task_id, run_id) = match k.create_run(&actor, &p.id, prompt, vec![], crate::kernel::NewRun { title: Some(format!("Undo {}", intent.tool)), ..Default::default() }).await {
        Ok(ids) => ids,
        Err(e) => return Ok(internal(e)),
    };
    let k_drive = k.clone();
    tokio::spawn(async move {
        if let Err(e) = k_drive.drive(&run_id).await {
            tracing::warn!("undo run {run_id} failed: {e:#}");
        }
    });
    Ok((StatusCode::CREATED, Json(json!({ "task_id": task_id, "undoes_call_id": call_id }))).into_response())
}

async fn explore_scan(State(k): K, Caller(p): Caller) -> ApiResult {
    match k.scan_for_ideas(&p.id).await {
        Ok(started) => Ok(Json(json!({ "started": started })).into_response()),
        Err(e) => Ok(err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))),
    }
}

/// Forget one of the person's own current memories. An archived one is not
/// theirs to see, so it is not found.
async fn forget(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    if !k.read(|s| s.is_current_memory(&p.id, &id)) {
        return refuse(StatusCode::NOT_FOUND, "no such memory");
    }
    Ok(crate::trash::trashed(k.trash(&p.id, rights(&p).moderates_shared_knowledge(), &id).await))
}

async fn health(State(k): K) -> ApiResult {
    // Down while the log's connection is lost: nothing can be recorded.
    let log = k.log.up().await;
    let body = Json(
        json!({ "ok": log, "log": if log { "up" } else { "down" }, "version": HARNESS_VERSION, "release": crate::update::version(), "kill_switch": k.read(|s| s.vault.kill_switch()) }),
    );
    Ok(if log { body.into_response() } else { (StatusCode::SERVICE_UNAVAILABLE, body).into_response() })
}

/// Speech to text on the appliance. The person reviews the text before sending it.
async fn transcribe(State(k): K, _: Caller, Body(b): Body<req::Transcribe>) -> ApiResult {
    let Some(data) = b.data_b64.as_deref() else { return refuse(StatusCode::BAD_REQUEST, "no audio") };
    Ok(match crate::speech::transcribe(k.cfg.speech.as_ref(), &k.cfg.appliance.state_dir, data).await {
        Ok(text) => Json(json!({ "text": text })).into_response(),
        Err(e) => err(StatusCode::SERVICE_UNAVAILABLE, format!("{e:#}")),
    })
}

/// Text to speech on the appliance, so every app and browser hears the same
/// voice. 503 when no voice is installed: the app then uses its system's own.
async fn speak(State(k): K, _: Caller, Body(b): Body<req::Speak>) -> ApiResult {
    let Some(cfg) = &k.cfg.read_aloud else { return refuse(StatusCode::SERVICE_UNAVAILABLE, "no read-aloud voice is installed on this server") };
    let Some(text) = b.text.as_deref().filter(|t| !t.trim().is_empty()) else { return refuse(StatusCode::BAD_REQUEST, "no text") };
    use crate::speech::SpeakError;
    Ok(match crate::speech::speak_limited(cfg, text).await {
        Ok(mp3) => Json(json!({ "media_type": "audio/mpeg", "audio_b64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, mp3) })).into_response(),
        Err(SpeakError::TooLong) => err(StatusCode::PAYLOAD_TOO_LARGE, format!("read aloud takes at most {} characters at a time", crate::speech::MAX_SPEAK)),
        Err(SpeakError::Busy) => err(StatusCode::TOO_MANY_REQUESTS, "the voice is busy; try again in a moment"),
        Err(SpeakError::TimedOut) => err(StatusCode::GATEWAY_TIMEOUT, "reading this aloud took too long"),
        Err(SpeakError::Failed(e)) => {
            tracing::warn!("read aloud: {e:#}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "read aloud failed")
        }
    })
}

/// Weak ETag derived from the kernel's monotonic log sequence number, scoped to the signed-in person.
pub(crate) fn seq_etag(k: &Kernel, person_id: &str) -> String {
    format!("W/\"seq-{}-{person_id}\"", k.read(|s| s.last_seq))
}

/// If the caller's `If-None-Match` already matches `etag`, return `304 Not Modified`.
pub(crate) fn not_modified(h: &HeaderMap, etag: &str) -> Option<Response> {
    let raw = h.get(axum::http::header::IF_NONE_MATCH)?.to_str().ok()?;
    let matched = raw.split(',').any(|part| {
        let p = part.trim();
        p == "*" || p == etag || p.strip_prefix("W/") == etag.strip_prefix("W/")
    });
    if !matched {
        return None;
    }
    let mut r = StatusCode::NOT_MODIFIED.into_response();
    if let Ok(v) = HeaderValue::from_str(etag) {
        r.headers_mut().insert(axum::http::header::ETAG, v);
    }
    r.headers_mut().insert(axum::http::header::CACHE_CONTROL, HeaderValue::from_static("private, no-cache"));
    r.headers_mut().insert(axum::http::header::VARY, HeaderValue::from_static("cookie, authorization"));
    Some(r)
}

/// Attach `ETag`, `Vary: cookie, authorization`, and `Cache-Control: private, no-cache` to a JSON response.
pub(crate) fn with_etag(mut r: Response, etag: &str) -> Response {
    if let Ok(v) = HeaderValue::from_str(etag) {
        r.headers_mut().insert(axum::http::header::ETAG, v);
    }
    r.headers_mut().insert(axum::http::header::CACHE_CONTROL, HeaderValue::from_static("private, no-cache"));
    r.headers_mut().insert(axum::http::header::VARY, HeaderValue::from_static("cookie, authorization"));
    r
}

#[cfg(test)]
mod same_origin_tests {
    use super::{not_modified, same_origin, with_etag};
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::IntoResponse;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, v.parse().unwrap());
        }
        h
    }

    /// Another site's page submitting the join form is refused; the
    /// server's own page, a typed address and a non-browser client are not.
    #[test]
    fn a_form_from_another_site_is_refused() {
        assert!(!same_origin(&headers(&[("sec-fetch-site", "cross-site")])));
        assert!(!same_origin(&headers(&[("sec-fetch-site", "same-site")])));
        assert!(!same_origin(&headers(&[("origin", "https://evil.example"), ("host", "magnus.example")])));
        assert!(same_origin(&headers(&[("sec-fetch-site", "same-origin")])));
        assert!(same_origin(&headers(&[("origin", "https://magnus.example:8443"), ("host", "magnus.example:8443")])));
        assert!(same_origin(&headers(&[])));
    }

    #[test]
    fn etag_not_modified_matches_weak_and_list_headers() {
        let etag = "W/\"seq-42-alice\"";
        assert!(not_modified(&headers(&[]), etag).is_none());
        assert!(not_modified(&headers(&[("if-none-match", "W/\"seq-42-bob\"")]), etag).is_none());
        let hit = not_modified(&headers(&[("if-none-match", "W/\"seq-42-alice\"")]), etag).unwrap();
        assert_eq!(hit.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(hit.headers().get("etag").unwrap(), etag);
        assert_eq!(hit.headers().get("vary").unwrap(), "cookie, authorization");
        assert!(not_modified(&headers(&[("if-none-match", "\"seq-1\", W/\"seq-42-alice\"")]), etag).is_some());
        let resp = with_etag(StatusCode::OK.into_response(), etag);
        assert_eq!(resp.headers().get("etag").unwrap(), etag);
        assert_eq!(resp.headers().get("cache-control").unwrap(), "private, no-cache");
        assert_eq!(resp.headers().get("vary").unwrap(), "cookie, authorization");
    }
}
