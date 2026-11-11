//! Operations in the Admin section (spec rev 141, Operations): "Updates
//! (channel, install, roll back), backups and restore checks, computers and
//! capacity, and health." Updates are the console's control routes. Each
//! backup is restored in memory as soon as it is written (`backup.rs`), and
//! that check is what "restore checks" shows; an admin can also back up now.
//! The channel is the desktop apps' and, in `server_channel.rs`, the server's.

use super::mount::{audited, body_json, control, pass, Audit};
use super::people::refuse;
use super::Admin;
use crate::kernel::Kernel;
use axum::extract::State;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Extension, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

/// How long the computer service may take to say its capacity before the
/// page shows it as not answering.
pub const CAPACITY_WAIT_SECONDS: u64 = 5;
/// A disk fuller than this is something an admin must see.
pub const DISK_ALERT_FRACTION: f64 = 0.85;

pub(super) fn routes() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/admin/updates", get(pass(None)))
        .route("/v1/admin/updates/install", post(pass(Some("update_installed"))))
        .route("/v1/admin/updates/rollback", post(pass(Some("update_rolled_back"))))
        .route("/v1/admin/updates/desktop-channel", put(pass(Some("desktop_channel_set"))))
        .route("/v1/admin/backups", get(backups).post(back_up))
        .route("/v1/admin/computers", get(computers))
        .route("/v1/admin/health", get(health))
        .merge(super::server_channel::routes())
}

async fn backups(State(k): State<Arc<Kernel>>) -> Response {
    Json(crate::backup::status(&k)).into_response()
}

async fn back_up(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>) -> Response {
    let audit = Audit { action: "backup_made".into(), target: Some("backups".into()), ..Default::default() };
    let change = async {
        match k.back_up(&a.actor()).await {
            Ok(v) => Json(v).into_response(),
            Err(e) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "failed", &format!("{e:#}")),
        }
    };
    audited(&k, &a, audit, change).await
}

/// The computer service's count, or why it did not answer.
pub async fn capacity(k: &Kernel) -> Value {
    let Some(socket) = k.cfg.computer.as_ref().map(|c| c.socket.clone()) else {
        return json!({ "set_up": false });
    };
    let mut v = match tokio::time::timeout(Duration::from_secs(CAPACITY_WAIT_SECONDS), k.computer_capacity()).await {
        Ok(Ok(v)) => json!({ "set_up": true, "answers": true, "live": v["live"], "max_live": v["max_live"], "computers": v["computers"], "max_computers": v["max_computers"] }),
        Ok(Err(e)) => json!({ "set_up": true, "answers": false, "error": e.to_string() }),
        Err(_) => json!({ "set_up": true, "answers": false, "error": "the computer service did not answer in time" }),
    };
    // Whether the service is as new as this server, as last asked.
    v["service"] = crate::computer::service_report(&k.cfg, &socket);
    v
}

/// Every computer, whose it is and what it is doing, and the machine's
/// capacity.
async fn computers(State(k): State<Arc<Kernel>>) -> Response {
    let list: Vec<Value> = k.read(|s| {
        s.computers
            .all()
            .map(|c| {
                let (person, workspace) = match &c.owner {
                    crate::computer::Owner::Person(p) => (Some(p.clone()), None),
                    crate::computer::Owner::Workspace(w) => (None, Some(w.clone())),
                };
                json!({ "id": c.id, "person": person, "workspace": workspace, "status": c.status, "controller": c.controller, "apps": c.apps.len() })
            })
            .collect()
    });
    Json(json!({ "computers": list, "capacity": capacity(&k).await })).into_response()
}

/// How full the disk holding the server's state is, as a fraction.
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
pub fn host(state_dir: &std::path::Path) -> Value {
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

/// Each service, the machine, and the latest errors.
async fn health(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>) -> Response {
    let (o, cap) = tokio::join!(control(&k, &a, Method::GET, "/v1/overview", None), capacity(&k));
    let o = body_json(o).await;
    let mut services = vec![json!({ "name": "Server", "up": true, "version": o["version"] }), json!({ "name": "Event log (Postgres)", "up": o["log_up"] == true })];
    if cap["set_up"] == true {
        let note = if cap["service"]["behind"] == true { cap["service"]["message"].clone() } else { cap["error"].clone() };
        services.push(json!({ "name": "Computers", "up": cap["answers"] == true, "note": note }));
    }
    Json(json!({
        "services": services,
        "host": host(&k.cfg.appliance.state_dir),
        "running_tasks": o["running_tasks"],
        "microvms_running": o["microvms_running"],
        "recent_errors": o["recent_errors"],
    }))
    .into_response()
}
