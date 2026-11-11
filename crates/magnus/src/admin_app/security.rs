//! Security in the Admin section (spec rev 141, Security): "Sign-in policy
//! (identity provider, two-factor, session length), the audit log with
//! export, how long chats, files and logs are kept, and the sessions where a
//! person granted developers access to their computer."
//!
//! The sign-in rules are the console's control route. How long things are
//! kept is shown as the server keeps them: the trash and backup windows, which
//! an admin sets within the purge window (`retention.rs`), and the log, which
//! is the permanent record.

use super::mount::{audited, body_json, control, Audit};
use super::Admin;
use crate::kernel::Kernel;
use axum::extract::{RawQuery, State};
use axum::http::Method;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

/// How long a deletion takes to reach backups and snapshots (spec, Files:
/// the purge window, the GDPR's month to act on an erasure).
pub const PURGE_WINDOW_DAYS: u32 = 30;

pub(super) fn routes() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/admin/sign-in-policy", get(policy).put(set_policy))
        .route("/v1/admin/audit", get(audit))
        .route("/v1/admin/audit/export", get(export))
        .route("/v1/admin/retention", get(retention))
        .route("/v1/admin/developer-access", get(developer_access))
        .merge(super::retention::routes())
        .merge(super::identity_provider::routes())
}

/// The sign-in rules, with the identity provider people sign in through.
async fn policy(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>) -> Response {
    let r = control(&k, &a, Method::GET, "/v1/admin/sign-in-policy", None).await;
    if !r.status().is_success() {
        return r;
    }
    let mut v = body_json(r).await;
    v["identity_provider"] = json!(k.oidc.get().map(|o| o.issuer().to_string()));
    Json(v).into_response()
}

async fn set_policy(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Json(mut b): Json<Value>) -> Response {
    // Who acts is the session's, never the client's to say.
    if let Some(o) = b.as_object_mut() {
        o.remove("actor");
    }
    let before = body_json(control(&k, &a, Method::GET, "/v1/admin/sign-in-policy", None).await).await;
    let audit = Audit { action: "sign_in_policy_changed".into(), target: Some("sign-in-policy".into()), before: Some(before), after: Some(b.clone()) };
    audited(&k, &a, audit, control(&k, &a, Method::PUT, "/v1/admin/sign-in-policy", Some(b))).await
}

async fn rows(k: &Arc<Kernel>, a: &Admin, q: Option<String>) -> Response {
    let path = match q {
        Some(q) if !q.is_empty() => format!("/v1/audit?{q}"),
        _ => "/v1/audit".to_string(),
    };
    control(k, a, Method::GET, &path, None).await
}

/// The audit, newest first, filtered by actor, action, target and time.
async fn audit(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, RawQuery(q): RawQuery) -> Response {
    rows(&k, &a, q).await
}

/// One CSV cell: a cell a spreadsheet would run as a formula is kept as text.
pub fn cell(v: &Value) -> String {
    let s = match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let s = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{s}") } else { s };
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// The audit as CSV, as the console exported it: `{filename, csv}`, so the
/// app saves it however it reaches the server.
pub fn csv(entries: &[Value]) -> String {
    let fields = ["seq", "at", "admin", "name", "action", "target", "source", "before", "after"];
    let mut out = format!("{}\r\n", fields.join(","));
    for r in entries {
        out.push_str(&fields.map(|f| cell(&r[f])).join(","));
        out.push_str("\r\n");
    }
    out
}

async fn export(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, RawQuery(q): RawQuery) -> Response {
    let r = rows(&k, &a, q).await;
    if !r.status().is_success() {
        return r;
    }
    let v = body_json(r).await;
    let entries = v["entries"].as_array().cloned().unwrap_or_default();
    let audit = Audit { action: "audit_exported".into(), target: Some("audit".into()), before: None, after: Some(json!({ "entries": entries.len() })) };
    let body = json!({ "filename": "magnus-audit.csv", "csv": csv(&entries) });
    // An export is a read, but one that leaves the server: it is recorded.
    audited(&k, &a, audit, async { Json(body).into_response() }).await
}

/// How long chats, files and logs are kept, as this server keeps them.
async fn retention(State(k): State<Arc<Kernel>>) -> Response {
    let (trash, backup) = (k.trash_keep_days(), k.backup_keep_days());
    Json(json!({
        "trash_days": trash,
        "backup_days": backup,
        "purge_window_days": PURGE_WINDOW_DAYS,
        "settable": super::retention::shown(&k),
        "kept": [
            { "what": "chats", "how": format!("Until the person deletes them; deleted, they wait in the trash for {trash} days, then leave the backups within {PURGE_WINDOW_DAYS} days.") },
            { "what": "files", "how": format!("Until the person deletes them; deleted, they leave the live volume at once and backups and snapshots within {PURGE_WINDOW_DAYS} days.") },
            { "what": "logs", "how": "The event log is the permanent record of what happened. A person's content in it is deleted when they or an admin delete it; the record that it happened stays.".to_string() },
            { "what": "backups", "how": format!("Each is kept {backup} days.") },
        ],
    }))
    .into_response()
}

/// Every session where a person let a developer look at their computer:
/// whose computer, who looked, until when, and whether it ended.
async fn developer_access(State(k): State<Arc<Kernel>>) -> Response {
    let rows: Vec<Value> = k.read(|s| {
        s.computers
            .all()
            .flat_map(|c| {
                let owner = match &c.owner {
                    crate::computer::Owner::Person(p) => json!({ "person": p }),
                    crate::computer::Owner::Workspace(w) => json!({ "workspace": w }),
                };
                c.operators.iter().map(move |o| json!({ "computer": c.id, "owner": owner, "session": o.session, "operator": o.operator, "until": o.until, "ended": o.ended }))
            })
            .collect()
    });
    let now = chrono::Utc::now();
    let open = rows.iter().filter(|r| r["ended"] == false && r["until"].as_str().and_then(|u| u.parse::<chrono::DateTime<chrono::Utc>>().ok()).is_some_and(|u| u > now)).count();
    Json(json!({ "sessions": rows, "open": open })).into_response()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn the_export_keeps_formulas_as_text_and_quotes_every_cell() {
        let out = super::csv(&[json!({ "seq": 3, "action": "=HYPERLINK(\"x\")", "after": { "a": 1 } })]);
        let mut lines = out.split("\r\n");
        assert_eq!(lines.next(), Some("seq,at,admin,name,action,target,source,before,after"));
        assert_eq!(lines.next(), Some("\"3\",\"\",\"\",\"\",\"'=HYPERLINK(\"\"x\"\")\",\"\",\"\",\"\",\"{\"\"a\"\":1}\""));
    }
}
