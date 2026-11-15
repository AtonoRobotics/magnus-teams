//! Slack's sign-in, whose one registered redirect is an https address:
//! the browser comes to the server, which sends it on to the desktop app.

use super::SLACK_HANDOFF;
use crate::kernel::Kernel;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::sync::Arc;

/// The desktop's loopback address with only this sign-in's answer. The code
/// is not written anywhere else.
pub(crate) fn slack_handoff_url(state: &str, code: Option<&str>, iss: Option<&str>, error: Option<&str>) -> String {
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    if let Some(c) = code.filter(|s| !s.is_empty()) {
        q.append_pair("code", c);
    }
    q.append_pair("state", state);
    if let Some(i) = iss.filter(|s| !s.is_empty()) {
        q.append_pair("iss", i);
    }
    if let Some(e) = error.filter(|s| !s.is_empty()) {
        q.append_pair("error", e);
    }
    format!("{SLACK_HANDOFF}?{}", q.finish())
}

/// Slack's browser comes here. A waiting sign-in is sent on to the desktop
/// app; anything else is an empty 404. The code is not logged.
pub(crate) async fn slack_oauth_callback(State(k): State<Arc<Kernel>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(state) = q.get("state").map(String::as_str).filter(|s| k.connecting.sign_ins.slack_open(s)) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let loc = slack_handoff_url(state, q.get("code").map(String::as_str), q.get("iss").map(String::as_str), q.get("error").map(String::as_str));
    let Ok(location) = loc.parse() else { return StatusCode::NOT_FOUND.into_response() };
    let mut r = Response::new(axum::body::Body::empty());
    *r.status_mut() = StatusCode::FOUND;
    r.headers_mut().insert(axum::http::header::LOCATION, location);
    r.headers_mut().insert(axum::http::header::CACHE_CONTROL, axum::http::HeaderValue::from_static("no-store"));
    r.headers_mut().insert(axum::http::header::REFERRER_POLICY, axum::http::HeaderValue::from_static("no-referrer"));
    r
}
