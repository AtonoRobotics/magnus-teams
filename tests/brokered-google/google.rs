//! The shared Google client's own tests (`brokered/google/src/lib.rs`, its
//! `tests` module), kept with the repository's tests.

use super::*;
use serde_json::json;

const API: Api = Api { host: "www.googleapis.com", base: "/calendar/v3", name: "Google Calendar", scope: "https://www.googleapis.com/auth/calendar" };

fn answer(status: u16, head: &str, body: Value) -> Response {
    Response { status, head: format!("HTTP/1.1 {status} X\r\n{head}"), body: body.to_string().into_bytes() }
}

/// Google's refusals are said as what the person or an admin does about
/// them, and a write with no clear answer is not said to have failed
/// (AtonoRobotics/magnus-claude#117's cases, as Google's REST APIs give
/// them).
#[test]
fn google_s_refusals_are_said_as_what_to_do() {
    let says = |f: Failure| match f {
        Failure::NotDone(t) => t,
        other => panic!("{other:?}"),
    };
    let gone = json!({ "error": { "code": 401, "message": "Request had invalid authentication credentials.", "status": "UNAUTHENTICATED" } });
    assert!(says(API.explain(&answer(401, "", gone), false)).contains("Google Calendar rejected the sign-in: it has expired or was revoked. Sign in again."));
    let off = json!({ "error": { "code": 403, "message": "Google Calendar API has not been used in project 123 before or it is disabled.", "errors": [{ "reason": "accessNotConfigured" }], "status": "PERMISSION_DENIED" } });
    assert!(says(API.explain(&answer(403, "", off), false)).contains("The Google Calendar API is not turned on for this Google Cloud project (accessNotConfigured)"));
    let scope = json!({ "error": { "code": 403, "message": "Request had insufficient authentication scopes.", "errors": [{ "reason": "insufficientPermissions" }], "details": [{ "reason": "ACCESS_TOKEN_SCOPE_INSUFFICIENT" }] } });
    let challenge = "WWW-Authenticate: Bearer realm=\"https://accounts.google.com/\", error=\"insufficient_scope\", scope=\"https://www.googleapis.com/auth/calendar.events https://www.googleapis.com/auth/calendar\"\r\n";
    assert!(matches!(API.explain(&answer(403, challenge, scope.clone()), true), Failure::NeedsScope(_, s) if s == ["https://www.googleapis.com/auth/calendar.events", API.scope]));
    assert!(matches!(API.explain(&answer(403, "", scope), true), Failure::NeedsScope(_, s) if s == [API.scope]));
    let busy = json!({ "error": { "code": 429, "errors": [{ "reason": "rateLimitExceeded" }] } });
    assert!(says(API.explain(&answer(429, "Retry-After: 30\r\n", busy), true)).contains("Try again shortly (in 30 seconds)"));
    assert!(says(API.explain(&answer(404, "", json!({})), false)).contains("Google Calendar could not find that"));
    assert!(matches!(API.explain(&answer(503, "", json!({})), true), Failure::Unknown(_)));
    assert!(says(API.explain(&answer(503, "", json!({})), false)).contains("HTTP 503"));
    let denied = json!({ "error": { "code": 403, "message": "You need to have writer access to this calendar.", "errors": [{ "reason": "requiredAccessLevel" }] } });
    assert!(says(API.explain(&answer(403, "", denied), true)).contains("refused this (HTTP 403, requiredAccessLevel: You need to have writer access"));
}

#[test]
fn a_query_carries_only_what_has_a_value() {
    assert_eq!(query(&[("q", Some("a b&c".into())), ("pageToken", None), ("maxResults", Some(String::new()))]), "?q=a%20b%26c");
    assert_eq!(enc("en.usa#holiday@group.v.calendar.google.com"), "en.usa%23holiday%40group.v.calendar.google.com");
}

/// An answer of success that is not JSON says nothing of what was done:
/// after a write it is an unknown outcome, never one that could be tried
/// again as if nothing happened (review of #126).
#[test]
fn an_answer_that_is_not_json_after_a_write_is_an_unknown_outcome() {
    assert!(matches!(API.json(b"<html>Accepted</html>", true), Err(Failure::Unknown(t)) if t.contains("whether it did this is unknown")));
    assert!(matches!(API.json(b"<html>Accepted</html>", false), Err(Failure::NotDone(t)) if t.contains("was not JSON")));
    assert_eq!(API.json(b" ", true).unwrap(), serde_json::json!({}));
    assert_eq!(API.json(b"{\"id\":\"e1\"}", true).unwrap(), serde_json::json!({ "id": "e1" }));
}
