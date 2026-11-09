//! Security › Sign-in in the admin console (admin console spec; owner's
//! ruling 1: no passkeys, people sign in with their password, and their
//! code when they use one): how members sign in (a code optional or
//! required), how admins sign in (shown, fixed), how long sessions last for
//! members (the kernel's) and for the console (its own), and when repeated
//! failures lock an account. Changes go to the kernel's sign-in rules,
//! which it enforces on the local path, with a fresh password.

use super::session::Admin;
use super::users::{failed, recorded, refuse};
use super::Console;
use axum::extract::State;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// The console takes lifetimes an admin set earlier (`ConsoleSettings` in
/// the log), when it starts.
pub fn adopt(c: &Console, v: &Value) {
    if let Some(m) = v["idle_minutes"].as_i64().filter(|m| (5..=240).contains(m)) {
        c.idle_minutes.store(m, Ordering::Relaxed);
    }
    if let Some(h) = v["session_hours"].as_i64().filter(|h| (1..=24).contains(h)) {
        c.session_hours.store(h, Ordering::Relaxed);
    }
}

async fn read(c: &Console) -> anyhow::Result<Value> {
    let p = c.kernel.call(Method::GET, "/v1/admin/sign-in-policy", None).await?;
    Ok(json!({
        "members": { "method": "password", "code": if p["code_required"] == true { "required" } else { "optional" } },
        "admins": { "method": "password", "code": "as members", "fixed": true },
        "sessions": {
            "members": { "idle_minutes": p["idle_minutes"], "max_hours": p["max_hours"] },
            "console": { "idle_minutes": c.idle_minutes.load(Ordering::Relaxed), "session_hours": c.session_hours.load(Ordering::Relaxed) },
        },
        "lockout": {
            "enabled": p["lockout_enabled"],
            "failures": p["lockout_failures"],
            "wait_minutes": p["lockout_wait_minutes"],
            "max_wait_minutes": p["lockout_max_wait_minutes"],
            "permanent": p["lockout_permanent"],
        },
    }))
}

pub async fn get(State(c): State<Arc<Console>>) -> Response {
    match read(&c).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => failed(e),
    }
}

fn int(v: &Value, range: std::ops::RangeInclusive<i64>, what: &str) -> Result<Option<i64>, Response> {
    match v {
        Value::Null => Ok(None),
        x => match x.as_i64().filter(|n| range.contains(n)) {
            Some(n) => Ok(Some(n)),
            None => Err(refuse(StatusCode::BAD_REQUEST, "invalid", &format!("{what} is a whole number from {} to {}.", range.start(), range.end()))),
        },
    }
}

/// Change the sign-in rules: a fresh password first. Only what is given
/// changes.
pub async fn patch(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Json(b): Json<Value>) -> Response {
    if let Err(r) = a.step_up() {
        return r;
    }
    let code = match b["members"]["code"].as_str() {
        None => None,
        Some(x @ ("optional" | "required")) => Some(x.to_string()),
        Some(_) => return refuse(StatusCode::BAD_REQUEST, "invalid", "A code is optional or required."),
    };
    let parsed = (|| -> Result<_, Response> {
        Ok((
            int(&b["sessions"]["members"]["idle_minutes"], 5..=1440, "Members' idle time")?,
            int(&b["sessions"]["members"]["max_hours"], 1..=720, "Members' longest session")?,
            int(&b["sessions"]["console"]["idle_minutes"], 5..=240, "The console's idle time")?,
            int(&b["sessions"]["console"]["session_hours"], 1..=24, "The console's longest session")?,
            int(&b["lockout"]["failures"], 1..=100, "Failures before a lockout")?,
            int(&b["lockout"]["wait_minutes"], 1..=1440, "The first wait")?,
            int(&b["lockout"]["max_wait_minutes"], 1..=10080, "The longest wait")?,
        ))
    })();
    let (m_idle, m_max, c_idle, c_hours, failures, wait, max_wait) = match parsed {
        Ok(x) => x,
        Err(r) => return r,
    };
    let before = match read(&c).await {
        Ok(v) => v,
        Err(e) => return failed(e),
    };
    if wait.zip(max_wait.or(before["lockout"]["max_wait_minutes"].as_i64())).is_some_and(|(w, m)| m < w)
        || max_wait.zip(wait.or(before["lockout"]["wait_minutes"].as_i64())).is_some_and(|(m, w)| m < w)
    {
        return refuse(StatusCode::BAD_REQUEST, "invalid", "The longest wait cannot be shorter than the first.");
    }
    // The kernel's own rules, given as what changes.
    let mut policy = serde_json::Map::new();
    if let Some(code) = &code {
        policy.insert("code_required".into(), json!(code == "required"));
    }
    for (field, v) in [("idle_minutes", m_idle), ("max_hours", m_max), ("lockout_failures", failures), ("lockout_wait_minutes", wait), ("lockout_max_wait_minutes", max_wait)] {
        if let Some(n) = v {
            policy.insert(field.into(), json!(n));
        }
    }
    for (from, to) in [(b["lockout"]["enabled"].as_bool(), "lockout_enabled"), (b["lockout"]["permanent"].as_bool(), "lockout_permanent")] {
        if let Some(on) = from {
            policy.insert(to.into(), json!(on));
        }
    }
    let console = c_idle.is_some() || c_hours.is_some();
    let r = recorded(&c, &a, "sign_in_rules_changed", "sign-in", Some(before.clone()), Some(b.clone()), async {
        if !policy.is_empty() {
            c.kernel.call(Method::PUT, "/v1/admin/sign-in-policy", Some(&Value::Object(policy))).await?;
        }
        if console {
            let idle = c_idle.unwrap_or(c.idle_minutes.load(Ordering::Relaxed));
            let hours = c_hours.unwrap_or(c.session_hours.load(Ordering::Relaxed));
            c.kernel.call(Method::PUT, "/v1/admin/console-settings", Some(&json!({ "actor": super::users::actor(&a), "idle_minutes": idle, "session_hours": hours }))).await?;
            c.idle_minutes.store(idle, Ordering::Relaxed);
            c.session_hours.store(hours, Ordering::Relaxed);
        }
        Ok(Value::Null)
    })
    .await;
    if let Err(r) = r {
        return r;
    }
    match read(&c).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => failed(e),
    }
}
