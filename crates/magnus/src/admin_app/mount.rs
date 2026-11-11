//! Mounting the console's control routes in the app: each is called
//! in-process, as the signed-in admin, so its checks and its events are the
//! console's own, not a copy. See the module documentation in `mod.rs`.

use super::Admin;
use crate::kernel::Kernel;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{header, Method, Request, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tower::ServiceExt;

/// Call the control route at `path` (with any query) as `a`: `body` with its
/// `actor` set to the admin, whatever the client sent. The control route's
/// answer is the answer.
pub async fn control(k: &Arc<Kernel>, a: &Admin, method: Method, path: &str, body: Option<Value>) -> Response {
    let mut body = body.unwrap_or_else(|| json!({}));
    let Some(fields) = body.as_object_mut() else { return refuse(StatusCode::BAD_REQUEST, "the body is not a JSON object") };
    fields.insert("actor".into(), json!(a.actor()));
    let req = match Request::builder().method(method).uri(path).header(header::CONTENT_TYPE, "application/json").body(Body::from(body.to_string())) {
        Ok(r) => r,
        Err(e) => return refuse(StatusCode::BAD_REQUEST, &format!("{e}")),
    };
    match crate::control::routes().with_state(k.clone()).oneshot(req).await {
        Ok(r) => r,
        Err(never) => match never {},
    }
}

/// What an audit entry says beyond who made it: the console's action name
/// (`user_disabled`, `kill_switch_released`, ...), what it acted on, and for a
/// change its before and after.
#[derive(Clone, Debug, Default)]
pub struct Audit {
    pub action: String,
    pub target: Option<String>,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

/// Write `audit` for `a` to the log through the control socket's own
/// `/v1/audit`: the entry's seq, or why it could not be written.
pub async fn record(k: &Arc<Kernel>, a: &Admin, audit: &Audit) -> Result<i64, String> {
    let entry = json!({ "admin": a.person.id, "name": a.name(), "action": audit.action, "target": audit.target, "source": a.source, "before": audit.before, "after": audit.after });
    let r = control(k, a, Method::POST, "/v1/audit", Some(entry)).await;
    let ok = r.status().is_success();
    let v = body_json(r).await;
    match v["seq"].as_i64() {
        Some(seq) if ok => Ok(seq),
        _ => Err(v["error"].as_str().unwrap_or("no answer").to_string()),
    }
}

/// The answer to a change refused because its audit entry was not written.
pub fn unaudited(why: &str) -> Response {
    (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "audit", "message": format!("The change was not made: its audit entry could not be written ({why}).") })))
        .into_response()
}

/// Make a change with the console's audit: the entry first (no entry, no
/// change), then the change; a change that fails is recorded again as
/// `<action>_failed` with what it answered.
pub async fn audited<F>(k: &Arc<Kernel>, a: &Admin, audit: Audit, change: F) -> Response
where
    F: Future<Output = Response>,
{
    if let Err(why) = record(k, a, &audit).await {
        return unaudited(&why);
    }
    let r = change.await;
    if r.status().is_success() {
        return r;
    }
    let status = r.status();
    let v = body_json(r).await;
    let failed = Audit { action: format!("{}_failed", audit.action), target: audit.target, before: None, after: Some(json!({ "status": status.as_u16(), "error": v["error"] })) };
    let _ = record(k, a, &failed).await;
    (status, Json(v)).into_response()
}

/// The most of a control route's answer read back: far above any it gives.
pub const CONTROL_ANSWER_MAX_BYTES: usize = 64 * 1024 * 1024;

type Answer = Pin<Box<dyn Future<Output = Response> + Send>>;

/// A handler that passes the request to the control route at the same path
/// and query, as the admin. With an `action`, a change (anything but GET) is
/// audited under it, targeting the path below `/v1/admin/` and recording the
/// body sent as its after; without one, nothing is audited beyond what the
/// control route records itself.
pub fn pass(action: Option<&'static str>) -> impl Fn(State<Arc<Kernel>>, Extension<Admin>, Method, Uri, Bytes) -> Answer + Clone + Send + Sync + 'static {
    move |State(k), Extension(a), method, uri, bytes| {
        Box::pin(async move {
            let mut body: Option<Value> = if bytes.is_empty() {
                None
            } else {
                match serde_json::from_slice(&bytes) {
                    Ok(v) => Some(v),
                    Err(_) => return refuse(StatusCode::BAD_REQUEST, "the body is not JSON"),
                }
            };
            // Who acts is the session's, never the client's to say.
            if let Some(fields) = body.as_mut().and_then(Value::as_object_mut) {
                fields.remove("actor");
            }
            let path = uri.path_and_query().map(|p| p.as_str().to_string()).unwrap_or_else(|| uri.path().to_string());
            match action.filter(|_| method != Method::GET) {
                Some(action) => {
                    let audit = Audit { action: action.into(), target: Some(uri.path().trim_start_matches("/v1/admin/").to_string()), before: None, after: body.clone() };
                    audited(&k, &a, audit, control(&k, &a, method, &path, body)).await
                }
                None => control(&k, &a, method, &path, body).await,
            }
        })
    }
}

/// A response's body as JSON, or null.
pub async fn body_json(r: Response) -> Value {
    match axum::body::to_bytes(r.into_body(), CONTROL_ANSWER_MAX_BYTES).await {
        Ok(b) => serde_json::from_slice(&b).unwrap_or(Value::Null),
        Err(_) => Value::Null,
    }
}

fn refuse(code: StatusCode, msg: &str) -> Response {
    (code, Json(json!({ "error": msg }))).into_response()
}
