//! Overview, Health and the kill switch in the admin console (admin console
//! spec, Overview, Operations; owner's ruling 10: the console's switch is
//! the app's own, through the kernel). Engaging never needs a step-up;
//! releasing does. Each change is in the audit log.

use super::session::Admin;
use super::Console;
use axum::extract::State;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde_json::{json, Value};
use std::sync::Arc;

fn kernel_down(e: anyhow::Error) -> Response {
    (StatusCode::BAD_GATEWAY, Json(json!({ "error": "kernel", "message": format!("{e:#}") }))).into_response()
}

pub async fn switch_state(State(c): State<Arc<Console>>) -> Response {
    match c.kernel.call(Method::GET, "/v1/kill-switch", None).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => kernel_down(e),
    }
}

/// Stop all agents: at once, with no step-up. It is recorded by the switch
/// itself even when nothing else can be; the audit entry follows.
pub async fn engage(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>) -> Response {
    let actor = format!("admin:{}", a.session.sub);
    let r = c.kernel.call(Method::POST, "/v1/kill-switch", Some(&json!({ "actor": actor, "engaged": true }))).await;
    let audited = c.audit((&a.session.sub, &a.session.name), "kill_switch_engaged", None, a.peer, None, r.as_ref().ok().cloned()).await;
    match (r, audited) {
        (Ok(v), Ok(_)) => Json(v).into_response(),
        (Ok(v), Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "audit", "message": format!("The kill switch is engaged, but its audit entry could not be written: {e:#}"), "switch": v })),
        )
            .into_response(),
        (Err(e), _) => kernel_down(e),
    }
}

/// Release it: only with a fresh password, recorded first.
pub async fn release(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>) -> Response {
    if let Err(r) = a.step_up() {
        return r;
    }
    if let Err(e) = c.audit((&a.session.sub, &a.session.name), "kill_switch_released", None, a.peer, Some(json!({ "engaged": true })), Some(json!({ "engaged": false }))).await {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "audit", "message": format!("{e:#}") }))).into_response();
    }
    match c.kernel.call(Method::POST, "/v1/kill-switch", Some(&json!({ "actor": format!("admin:{}", a.session.sub), "engaged": false }))).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => kernel_down(e),
    }
}

/// How full the disk holding the appliance's state is, as a fraction.
fn disk_used(path: &std::path::Path) -> Option<f64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `s` a writable statvfs.
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 || s.f_blocks == 0 {
        return None;
    }
    Some(1.0 - (s.f_bavail as f64 / s.f_blocks as f64))
}

/// The machine: load, memory and disk.
fn host(state_dir: &std::path::Path) -> Value {
    let load = std::fs::read_to_string("/proc/loadavg").ok().and_then(|l| l.split_whitespace().next().and_then(|x| x.parse::<f64>().ok()));
    let mem = std::fs::read_to_string("/proc/meminfo").ok().map(|m| {
        let kb = |k: &str| m.lines().find(|l| l.starts_with(k)).and_then(|l| l.split_whitespace().nth(1)).and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0);
        let (total, avail) = (kb("MemTotal:"), kb("MemAvailable:"));
        if total > 0.0 {
            1.0 - avail / total
        } else {
            0.0
        }
    });
    json!({ "cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1), "load": load, "memory_used": mem, "disk_used": disk_used(state_dir) })
}

/// Each certificate the appliance serves: what it is for, when it expires,
/// and how many whole days are left.
pub fn certificates(c: &Console) -> Vec<Value> {
    let now = chrono::Utc::now();
    c.certs
        .iter()
        .map(|(what, path)| {
            let end = super::cert::not_after(path);
            json!({ "what": what, "path": path, "not_after": end, "days_left": end.map(|e| (e - now).num_days()), "readable": end.is_some() })
        })
        .collect()
}

/// The status strip, the cards and the alerts, each alert naming where it
/// is fixed.
pub async fn overview(State(c): State<Arc<Console>>) -> Response {
    let providers = async { tokio::time::timeout(std::time::Duration::from_secs(6), c.kernel.call(Method::GET, "/v1/admin/providers", None)).await.ok().and_then(Result::ok) };
    let (k, tunnel, providers) = tokio::join!(c.kernel.call(Method::GET, "/v1/overview", None), tunnel_up(&c), providers);
    let mut alerts_extra = vec![];
    if tunnel == Some(false) {
        alerts_extra.push(json!({ "what": "The tunnel is not connected: people outside the organisation's network cannot reach Magnus.", "fix": "#/operations/network" }));
    }
    for p in providers.as_ref().and_then(|v| v["providers"].as_array()).into_iter().flatten() {
        if p["enabled"] == true && p["answers"] != true {
            alerts_extra.push(json!({ "what": format!("The model provider {} is not answering: {}", p["id"].as_str().unwrap_or_default(), p["error"].as_str().unwrap_or("no answer")), "fix": "#/operations/providers" }));
        }
    }
    let h = host(&c.state_dir);
    let mut alerts = vec![];
    match &k {
        Err(_) => alerts.push(json!({ "what": "The kernel is not answering.", "fix": "#/operations/health" })),
        Ok(v) if v["log_up"] != true => alerts.push(json!({ "what": "The event log (Postgres) is not answering.", "fix": "#/operations/health" })),
        _ => {}
    }
    if h["disk_used"].as_f64().is_some_and(|d| d > 0.85) {
        alerts.push(json!({ "what": format!("The disk is {:.0}% full.", h["disk_used"].as_f64().unwrap_or(0.0) * 100.0), "fix": "#/operations/health" }));
    }
    alerts.extend(alerts_extra);
    if let Some(e) = c.tls.error.read().unwrap().clone() {
        alerts.push(json!({ "what": format!("The console's trusted certificate could not be issued: {e}"), "fix": "#/operations/network" }));
    }
    let certs = certificates(&c);
    for x in &certs {
        if let Some(days) = x["days_left"].as_i64().filter(|d| *d < 14) {
            let what = x["what"].as_str().unwrap_or_default();
            let when = if days < 0 {
                "has expired".to_string()
            } else if days == 0 {
                "expires today".to_string()
            } else {
                format!("expires in {days} day{}", if days == 1 { "" } else { "s" })
            };
            alerts.push(json!({ "what": format!("{what} {when}."), "fix": "#/operations/network" }));
        }
    }
    let k = k.unwrap_or(Value::Null);
    Json(json!({
        "certificates": certs,
        "status": { "healthy": alerts.is_empty(), "kill_switch": k["kill_switch"], "version": k["version"], "tunnel": tunnel },
        "cards": { "running_tasks": k["running_tasks"], "waiting_tasks": k["waiting_tasks"], "signed_in": k["signed_in"], "knowledge_changes_24h": k["knowledge_changes_24h"] },
        "alerts": alerts,
    }))
    .into_response()
}

/// Each service, the machine, and the latest errors.
pub async fn health(State(c): State<Arc<Console>>) -> Response {
    let (k, tunnel) = tokio::join!(c.kernel.call(Method::GET, "/v1/overview", None), tunnel_up(&c));
    let kernel_up = k.is_ok();
    let k = k.unwrap_or(Value::Null);
    let mut services =
        vec![json!({ "name": "Kernel", "up": kernel_up }), json!({ "name": "Event log (Postgres)", "up": k["log_up"] == true }), json!({ "name": "Admin console", "up": true })];
    match tunnel {
        Some(up) => services.push(json!({ "name": "Tunnel (cloudflared)", "up": up })),
        None => services.push(json!({ "name": "Tunnel (cloudflared)", "up": null, "note": "not set up on this server" })),
    }
    Json(json!({
        "services": services,
        "microvms_running": k["microvms_running"],
        "host": host(&c.state_dir),
        "running_tasks": k["running_tasks"],
        "recent_errors": k["recent_errors"],
    }))
    .into_response()
}

// The rest of Operations (admin console spec): Modules, Model providers,
// Updates and Network. Each change is audited before it is made; installing
// an update, rolling one back and changing the network allowlist need a
// fresh password.

use super::users::{actor, failed, recorded, refuse};

async fn kernel_get(c: &Console, path: &str) -> Response {
    match c.kernel.call(Method::GET, path, None).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => failed(e),
    }
}

fn plain_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 100 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub async fn modules(State(c): State<Arc<Console>>) -> Response {
    kernel_get(&c, "/v1/admin/modules").await
}

/// Turn a module on or off for the organisation (`on`: true, false, or
/// null for the config's).
pub async fn set_module(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, axum::extract::Path(id): axum::extract::Path<String>, Json(b): Json<Value>) -> Response {
    if !plain_id(&id) {
        return refuse(StatusCode::NOT_FOUND, "not_found", "No such module.");
    }
    let before = c
        .kernel
        .call(Method::GET, "/v1/admin/modules", None)
        .await
        .ok()
        .and_then(|v| v["modules"].as_array().and_then(|m| m.iter().find(|x| x["name"] == id.as_str()).map(|x| json!({ "on": x["on"], "set_by_admin": x["set_by_admin"] }))));
    let body = json!({ "actor": actor(&a), "on": b["on"] });
    match recorded(&c, &a, "module_set", &id, before, Some(json!({ "on": b["on"] })), c.kernel.call(Method::POST, &format!("/v1/admin/modules/{id}"), Some(&body))).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// Modules are part of Magnus's own release: there is no package to install
/// or remove apart from a release, which Updates installs. The spec's open
/// question 22 (what "module" names) is not ruled; until it is, both answer
/// 409.
pub async fn module_package() -> Response {
    refuse(StatusCode::CONFLICT, "not_packaged", "Magnus's modules come with its release and are turned on or off here. A new module arrives with an update.")
}

pub async fn providers(State(c): State<Arc<Console>>) -> Response {
    kernel_get(&c, "/v1/admin/providers").await
}

pub async fn test_provider(State(c): State<Arc<Console>>, axum::extract::Path(id): axum::extract::Path<String>) -> Response {
    if !plain_id(&id) {
        return refuse(StatusCode::NOT_FOUND, "not_found", "No such provider.");
    }
    match c.kernel.call(Method::POST, &format!("/v1/admin/providers/{id}/test"), Some(&json!({}))).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => failed(e),
    }
}

/// Enable or disable a provider, or make one of its models the default.
pub async fn set_provider(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, axum::extract::Path(id): axum::extract::Path<String>, Json(b): Json<Value>) -> Response {
    if !plain_id(&id) {
        return refuse(StatusCode::NOT_FOUND, "not_found", "No such provider.");
    }
    let after = json!({ "enabled": b["enabled"], "default_model": b["default_model"] });
    let body = json!({ "actor": actor(&a), "enabled": b["enabled"], "default_model": b["default_model"] });
    match recorded(&c, &a, "provider_set", &id, None, Some(after), c.kernel.call(Method::PATCH, &format!("/v1/admin/providers/{id}"), Some(&body))).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

pub async fn updates(State(c): State<Arc<Console>>) -> Response {
    kernel_get(&c, "/v1/admin/updates").await
}

/// Install the available update now: a fresh password first.
pub async fn install(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>) -> Response {
    if let Err(r) = a.step_up() {
        return r;
    }
    match recorded(
        &c,
        &a,
        "update_installed",
        "appliance",
        Some(json!({ "version": crate::update::version() })),
        None,
        c.kernel.call(Method::POST, "/v1/admin/updates/install", Some(&json!({ "actor": actor(&a) }))),
    )
    .await
    {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// Go back to the release kept: a fresh password first.
pub async fn rollback(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>) -> Response {
    if let Err(r) = a.step_up() {
        return r;
    }
    match recorded(
        &c,
        &a,
        "update_rolled_back",
        "appliance",
        Some(json!({ "version": crate::update::version() })),
        None,
        c.kernel.call(Method::POST, "/v1/admin/updates/rollback", Some(&json!({ "actor": actor(&a) }))),
    )
    .await
    {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// The channel everyone but the owner and testers follows.
pub async fn desktop_channel(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Json(b): Json<Value>) -> Response {
    let body = json!({ "actor": actor(&a), "channel": b["channel"] });
    match recorded(
        &c,
        &a,
        "desktop_channel_set",
        "desktop",
        None,
        Some(json!({ "channel": b["channel"] })),
        c.kernel.call(Method::PUT, "/v1/admin/updates/desktop-channel", Some(&body)),
    )
    .await
    {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// Whether the tunnel says it is connected, if there is one.
async fn tunnel_up(c: &Console) -> Option<bool> {
    let url = c.tunnel_ready.as_ref()?;
    let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(3)).no_proxy().build().ok()?;
    Some(matches!(tokio::time::timeout(std::time::Duration::from_secs(3), http.get(url).send()).await, Ok(Ok(r)) if r.status().is_success()))
}

/// The console takes an allowlist an admin set before this start.
pub fn adopt_allow(c: &Console, v: &Value) {
    let Some(list) = v["allow"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<String>>()) else { return };
    match super::net::allowlist(&list) {
        Ok(subnets) => {
            *c.allow.write().unwrap() = subnets;
            *c.allow_text.write().unwrap() = list;
        }
        Err(e) => tracing::error!("admin console: the allowlist an admin set is not usable, keeping the config's: {e:#}"),
    }
}

/// The network: the tunnel and its public names, the console's own name
/// and addresses, the subnets it takes connections from, and the
/// certificates with when each ends.
pub async fn network(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>) -> Response {
    let tunnel = tunnel_up(&c).await;
    Json(json!({
        "tunnel": { "configured": c.tunnel_ready.is_some(), "connected": tunnel, "public_hostnames": c.public_hostnames },
        "admin": { "hostname": c.hostname, "aliases": c.aliases, "listen": c.listen, "public": false },
        "allow": *c.allow_text.read().unwrap(),
        "you": a.peer.ip().to_string(),
        "certificates": certificates(&c),
        "trusted": trusted(&c),
    }))
    .into_response()
}

/// How the console's trusted certificate is had, whether browsers naming
/// its hostname get it yet, and why not.
fn trusted(c: &Console) -> Value {
    json!({ "via": c.tls.via(), "serving": c.tls.certs.has_trusted(), "error": *c.tls.error.read().unwrap() })
}

/// Operations › Network › Renew: the organisation's certificate read again,
/// and the trusted one issued or renewed; served from the next connection.
/// Recorded first; a failure is recorded with what was said.
pub async fn renew_certificate(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>) -> Response {
    let ends = |c: &Console| json!(certificates(c).iter().map(|x| json!({ "what": x["what"], "not_after": x["not_after"] })).collect::<Vec<_>>());
    let before = json!({ "certificates": ends(&c) });
    let renew = async { c.tls.renew().await.map(|_| Value::Null) };
    match recorded(&c, &a, "certificate_renewed", "certificates", Some(before), None, renew).await {
        Ok(_) => Json(json!({ "certificates": certificates(&c), "trusted": trusted(&c) })).into_response(),
        Err(r) => r,
    }
}

/// Change the subnets the console takes connections from: a fresh password
/// first, never loopback, and never one that would shut out the admin's
/// own address.
pub async fn set_allowlist(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Json(b): Json<Value>) -> Response {
    if let Err(r) = a.step_up() {
        return r;
    }
    let Some(list) = b["allow"].as_array().map(|x| x.iter().filter_map(|s| s.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect::<Vec<String>>()) else {
        return refuse(StatusCode::BAD_REQUEST, "invalid", "Give the subnets, such as 192.168.0.0/24.");
    };
    let subnets = match super::net::allowlist(&list) {
        Ok(s) => s,
        Err(e) => return refuse(StatusCode::BAD_REQUEST, "invalid", &format!("{e:#}")),
    };
    if !super::net::allowed(&subnets, a.peer.ip()) {
        return refuse(
            StatusCode::CONFLICT,
            "shuts_you_out",
            &format!("That list leaves out your own address, {}: you would lose the console. Keep a subnet that holds it.", a.peer.ip()),
        );
    }
    let before = json!({ "allow": *c.allow_text.read().unwrap() });
    let body = json!({ "actor": actor(&a), "allow": list });
    match recorded(
        &c,
        &a,
        "network_allowlist_changed",
        "allowlist",
        Some(before),
        Some(json!({ "allow": list })),
        c.kernel.call(Method::PUT, "/v1/admin/console-settings", Some(&body)),
    )
    .await
    {
        Ok(_) => {
            *c.allow.write().unwrap() = subnets;
            *c.allow_text.write().unwrap() = list.clone();
            Json(json!({ "allow": list })).into_response()
        }
        Err(r) => r,
    }
}
