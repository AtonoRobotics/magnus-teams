//! Slack's sign-in, whose one registered redirect is an https address:
//! the browser comes to the server, which sends it on to the desktop app.

use super::sign_in::pending;
use super::{PENDING_FOR, SLACK_HANDOFF, SLACK_OAUTH_CALLBACK};
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;

/// Whether `state` is a Slack sign-in still waiting, started with the
/// registered https redirect. The pending entry stays: the desktop finishes it.
pub(crate) fn slack_sign_in_open(state: &str) -> bool {
    pending().lock().unwrap_or_else(|e| e.into_inner()).get(state).is_some_and(|p| p.app == "slack" && p.redirect_uri == SLACK_OAUTH_CALLBACK && p.at.elapsed() < PENDING_FOR)
}

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
pub(crate) async fn slack_oauth_callback(Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(state) = q.get("state").map(String::as_str).filter(|s| slack_sign_in_open(s)) else {
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
