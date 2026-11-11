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
    let mut alerts = alerts(&o, &k, host["disk_used"].as_f64(), &capacity);
    let spend = k.spend_overview(None).map(|s| spend_summary(&s)).unwrap_or(Value::Null);
    alerts.extend(spend_alerts(&spend));
    let invited = {
        let mut n = 0;
        for id in k.server_people() {
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
        "people": { "total": k.server_people().len(), "invited": invited },
        "kill_switch": o["kill_switch"],
        "recent_errors": o["recent_errors"],
        "alerts": alerts,
        "spend": spend,
    }))
    .into_response()
}

/// This month's spend against its limits, from Admin → Spend's overview:
/// the total, the server's limit and what is left, who has reached their
/// own limit, and requests for more still waiting.
fn spend_summary(s: &Value) -> Value {
    let at_limit: Vec<Value> = s["by_person"].as_array().into_iter().flatten().filter(|p| p["left_usd"].as_f64().is_some_and(|l| l <= 0.0)).map(|p| p["name"].clone()).collect();
    let waiting = s["requests"].as_array().into_iter().flatten().filter(|r| r["decided"].is_null()).count();
    json!({
        "month": s["month"],
        "total_usd": s["total_usd"],
        "server_usd": s["limits"]["server_usd"],
        "server_left_usd": s["server_left_usd"],
        "at_limit": at_limit,
        "requests_waiting": waiting,
    })
}

/// What in this month's spend needs an admin, each fixed in Spend.
fn spend_alerts(s: &Value) -> Vec<Value> {
    let mut out = vec![];
    if s["server_left_usd"].as_f64().is_some_and(|l| l <= 0.0) {
        out.push(alert("The server's monthly spend limit is reached: no new work starts.".into(), "spend"));
    }
    let n = s["at_limit"].as_array().map_or(0, Vec::len);
    if n > 0 {
        out.push(alert(format!("{n} {} reached their spend limit.", if n == 1 { "person has" } else { "people have" }), "spend"));
    }
    match s["requests_waiting"].as_u64().unwrap_or(0) {
        0 => {}
        1 => out.push(alert("A request for more spend is waiting.".into(), "spend")),
        w => out.push(alert(format!("{w} requests for more spend are waiting."), "spend")),
    }
    out
}

#[cfg(test)]
mod spend_card_tests {
    use super::{spend_alerts, spend_summary};
    use serde_json::json;

    /// Overview shows the month against the server's limit, and a reached
    /// limit or a waiting request is something that needs an admin.
    #[test]
    fn overview_carries_spend_and_its_alerts() {
        let s = json!({ "month": "2026-11", "total_usd": 40.0, "server_left_usd": 0.0, "limits": { "server_usd": 40.0 },
            "by_person": [{ "name": "Ada", "left_usd": 0.0 }, { "name": "Bea", "left_usd": 3.0 }, { "name": "Cy", "left_usd": null }],
            "requests": [{ "decided": null }, { "decided": "approved" }] });
        let sum = spend_summary(&s);
        assert_eq!(sum["total_usd"], 40.0);
        assert_eq!(sum["server_usd"], 40.0);
        assert_eq!(sum["at_limit"], json!(["Ada"]));
        assert_eq!(sum["requests_waiting"], 1);
        let alerts = spend_alerts(&sum);
        assert_eq!(alerts.len(), 3, "{alerts:?}");
        assert!(alerts.iter().all(|a| a["fix"] == "spend"), "{alerts:?}");
        let quiet = spend_summary(&json!({ "server_left_usd": 10.0, "by_person": [], "requests": [] }));
        assert!(spend_alerts(&quiet).is_empty());
    }
}
