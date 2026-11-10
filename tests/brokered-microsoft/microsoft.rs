//! The shared Microsoft Graph client's own tests
//! (`brokered/microsoft/src/lib.rs`, its `tests` module), kept with the
//! repository's tests.

use super::*;
use serde_json::json;

const MAIL: Graph = Graph { name: "Outlook", scopes: &["https://graph.microsoft.com/Mail.ReadWrite", "https://graph.microsoft.com/Mail.Send"] };

fn answer(status: u16, head: &str, body: Value) -> Response {
    Response { status, head: format!("HTTP/1.1 {status} X\r\n{head}"), body: body.to_string().into_bytes() }
}

fn says(f: Failure) -> String {
    match f {
        Failure::NotDone(t) => t,
        other => panic!("{other:?}"),
    }
}

/// Graph's refusals are said as what the person or an admin does about
/// them, and a write with no clear answer is not said to have failed.
#[test]
fn graph_s_refusals_are_said_as_what_to_do() {
    let gone = json!({ "error": { "code": "InvalidAuthenticationToken", "message": "Lifetime validation failed, the token is expired." } });
    assert!(says(MAIL.explain(&answer(401, "", gone), false)).contains("Outlook rejected the sign-in: it has expired or was revoked. Sign in again."));
    let denied = json!({ "error": { "code": "ErrorAccessDenied", "message": "Access is denied. Check credentials and try again." } });
    match MAIL.explain(&answer(403, "", denied), true) {
        Failure::NeedsScope(t, s) => {
            assert!(t.contains("an admin of the organisation allows it"), "{t}");
            assert_eq!(s, MAIL.scopes);
        }
        other => panic!("{other:?}"),
    }
    let no_mailbox = json!({ "error": { "code": "MailboxNotEnabledForRESTAPI", "message": "The mailbox is either inactive, soft-deleted, or is hosted on-premise." } });
    assert!(says(MAIL.explain(&answer(404, "", no_mailbox), false)).contains("no Exchange Online mailbox"));
    let busy = json!({ "error": { "code": "TooManyRequests", "message": "Please retry again later." } });
    assert!(says(MAIL.explain(&answer(429, "Retry-After: 12\r\n", busy), true)).contains("Try again shortly (in 12 seconds)"));
    assert!(says(MAIL.explain(&answer(404, "", json!({ "error": { "code": "ErrorItemNotFound" } })), false)).contains("Outlook could not find that"));
    assert!(matches!(MAIL.explain(&answer(503, "", json!({})), true), Failure::Unknown(_)));
    assert!(says(MAIL.explain(&answer(503, "", json!({})), false)).contains("HTTP 503"));
    let exists = json!({ "error": { "code": "nameAlreadyExists", "message": "The specified item name already exists." } });
    assert!(says(MAIL.explain(&answer(409, "", exists), true)).contains("refused this (HTTP 409, nameAlreadyExists: The specified item name already exists."));
}

/// An answer of success that is not JSON says nothing of what was done:
/// after a write it is an unknown outcome.
#[test]
fn an_answer_that_is_not_json_after_a_write_is_an_unknown_outcome() {
    assert!(matches!(MAIL.json(b"<html>Accepted</html>", true), Err(Failure::Unknown(t)) if t.contains("whether it did this is unknown")));
    assert!(matches!(MAIL.json(b"<html>Accepted</html>", false), Err(Failure::NotDone(t)) if t.contains("was not JSON")));
    assert_eq!(MAIL.json(b"", true).unwrap(), json!({}));
}

/// A page token is the next page Graph named, and only a path under the
/// tool's own: never another address.
#[test]
fn a_page_token_continues_only_the_tool_s_own_request() {
    let v = json!({ "@odata.nextLink": "https://graph.microsoft.com/v1.0/me/messages?$top=10&$skip=10" });
    let next = next_page(&v).unwrap();
    assert_eq!(next, "/v1.0/me/messages?$top=10&$skip=10");
    assert_eq!(page(&next, "/v1.0/me/").unwrap(), next);
    assert!(page("/v1.0/users/someone/messages", "/v1.0/me/").is_err());
    assert!(page("/v1.0/me/../users/x", "/v1.0/me/").is_err());
    assert_eq!(next_page(&json!({ "@odata.nextLink": "https://elsewhere.example/v1.0/me/x" })), None);
}

#[test]
fn queries_and_people_are_written_as_graph_takes_them() {
    assert_eq!(query(&[("$search", Some("\"from:bo\"".into())), ("$top", None), ("$skip", Some(String::new()))]), "?$search=%22from%3Abo%22");
    assert_eq!(id(&json!({ "message_id": "AAMk/x=" }), "message_id").unwrap(), "AAMk%2Fx%3D");
    assert!(id(&json!({}), "message_id").is_err());
    let p = people(&["bo@example.com".to_string()]);
    assert_eq!(p, json!([{ "emailAddress": { "address": "bo@example.com" } }]));
    assert_eq!(addresses(&p), ["bo@example.com"]);
}
