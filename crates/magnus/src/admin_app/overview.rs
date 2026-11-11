//! Overview in the Admin section (spec rev 141, Overview): "What is running
//! now, spend this month against its limits, computers live against
//! capacity, and anything that needs an admin: an agent stopped, a limit
//! reached, a failed backup or update."
//!
//! Spend, and the stops below the server's, are other sections': this
//! answer carries neither, and the app's Overview draws the cards other
//! sections register with `registerOverviewCard`
//! (desktop/ui/admin-overview.js), so the month's spend, a reached limit and
//! a stopped agent appear here once Spend and the kill switch levels
//! register them. Each alert names the section where it is fixed.

use super::mount::{body_json, control};
use super::Admin;
use crate::kernel::Kernel;
use axum::extract::State;
use axum::http::Method;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) fn routes() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/admin/overview", get(overview))
}

/// One thing that needs an admin: what, and the section that fixes it.
fn alert(what: String, fix: &str) -> Value {
    json!({ "what": what, "fix": fix })
}

/// What needs an admin, from the kernel's own record.
pub fn alerts(o: &Value, k: &Kernel, disk: Option<f64>, capacity: &Value) -> Vec<Value> {
    let mut out = vec![];
    if o["log_up"] != true {
        out.push(alert("The event log (Postgres) is not answering.".into(), "operations"));
    }
    if o["kill_switch"] == true {
        out.push(alert("The kill switch is on: every agent is stopped.".into(), "overview"));
    }
    let (backup, copy, scrub, update) = k.read(|s| (s.last_backup.clone(), s.last_copy.clone(), s.scrub_pending.clone(), s.last_update.clone()));
    if let Some((file, false, detail, _)) = backup {
        out.push(alert(format!("The latest backup ({file}) did not restore: {detail}"), "operations"));
    }
    if let Some((_, false, detail, _)) = copy {
        out.push(alert(format!("The latest backup was not copied off the server: {detail}"), "operations"));
    }
    if let Some(p) = scrub {
        out.push(alert(format!("{} backups still hold deleted content: {}", p.files.len(), p.detail), "operations"));
    }
    if let Some(u) = update.filter(|u| u.what == "failed") {
        out.push(alert(format!("The update to {} failed: {}", u.to, u.detail), "operations"));
    }
    if let Some(d) = disk.filter(|d| *d > super::operations::DISK_ALERT_FRACTION) {
        out.push(alert(format!("The disk is {:.0}% full.", d * 100.0), "operations"));
    }
    if capacity["answers"] == false {
        out.push(alert(format!("The computer service is not answering: {}", capacity["error"].as_str().unwrap_or("no answer")), "operations"));
    } else if capacity["live"].as_u64().zip(capacity["max_live"].as_u64()).is_some_and(|(l, m)| m > 0 && l >= m) {
        out.push(alert("Every computer this server can run is running: new ones wait.".into(), "operations"));
    }
    out
}

async fn overview(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>) -> Response {
    let (o, capacity) = tokio::join!(control(&k, &a, Method::GET, "/v1/overview", None), super::operations::capacity(&k));
    let o = body_json(o).await;
    let host = super::operations::host(&k.cfg.appliance.state_dir);
    let alerts = alerts(&o, &k, host["disk_used"].as_f64(), &capacity);
    let invited = {
        let mut n = 0;
        for id in k.everyone() {
            if k.accounts.status(&id).await == crate::accounts::Status::Invited {
                n += 1;
            }
        }
        n
    };
    Json(json!({
        "version": o["version"],
        "running": { "tasks": o["running_tasks"], "waiting": o["waiting_tasks"], "signed_in": o["signed_in"], "computers": o["microvms_running"] },
        "computers": capacity,
        "people": { "total": k.everyone().len(), "invited": invited },
        "kill_switch": o["kill_switch"],
        "recent_errors": o["recent_errors"],
        "alerts": alerts,
        "spend": Value::Null,
    }))
    .into_response()
}
