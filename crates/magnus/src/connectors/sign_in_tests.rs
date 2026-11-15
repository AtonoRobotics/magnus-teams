//! The sign-in's own checks, each without the network: the address it
//! sends the person to, the answer's issuer, the token request, who may
//! finish a waiting sign-in, and what the token answer keeps.

use super::*;
use std::time::Instant;

fn client(resource: &str, secret: Option<&str>) -> Client {
    Client {
        token_endpoint: "https://auth.example.com/token".into(),
        revocation_endpoint: None,
        client_id: "magnus-client".into(),
        client_secret: secret.map(String::from),
        resource: resource.into(),
        relay: None,
    }
}

fn waiting(app: &str, person: &str, pkce: bool, resource: &str, at: Instant) -> Pending {
    Pending {
        app: app.into(),
        person: person.into(),
        verifier: "the-verifier".into(),
        redirect_uri: "http://127.0.0.1:4711/callback".into(),
        client: client(resource, None),
        issuer: "https://auth.example.com".into(),
        iss_required: false,
        pkce,
        kept: None,
        at,
    }
}

fn query(u: &url::Url) -> HashMap<String, String> {
    u.query_pairs().into_owned().collect()
}

/// The address carries the PKCE challenge of this sign-in's verifier
/// (S256), the resource it is for, and the scopes, each only when there
/// is one.
#[test]
fn the_sign_in_address_carries_the_challenge_and_the_resource() {
    let scopes = vec!["read".to_string(), "write".to_string()];
    let ask = Authorize {
        endpoint: "https://auth.example.com/authorize",
        redirect_uri: "http://127.0.0.1:4711/callback",
        state: "st",
        verifier: Some("the-verifier"),
        resource: "https://mcp.example.com/mcp",
        scopes: &scopes,
    };
    let q = query(&authorize_url(&ask, "magnus-client").unwrap());
    assert_eq!(q["code_challenge"], challenge("the-verifier"), "the challenge is not the verifier's");
    assert_ne!(q["code_challenge"], "the-verifier", "the verifier itself went in the address");
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["resource"], "https://mcp.example.com/mcp");
    assert_eq!((q["response_type"].as_str(), q["client_id"].as_str(), q["state"].as_str()), ("code", "magnus-client", "st"));
    assert_eq!(q["scope"], "read write");
    assert!(!q.contains_key("access_type"));

    let plain = Authorize { verifier: None, resource: "", scopes: &[], endpoint: "https://accounts.google.com/o/oauth2/auth", ..ask };
    let q = query(&authorize_url(&plain, "c").unwrap());
    assert!(!q.contains_key("code_challenge") && !q.contains_key("code_challenge_method"), "a challenge without PKCE: {q:?}");
    assert!(!q.contains_key("resource") && !q.contains_key("scope"), "{q:?}");
    assert_eq!((q["access_type"].as_str(), q["prompt"].as_str()), ("offline", "consent"), "Google's lasting refresh token");
}

#[test]
fn the_scopes_are_the_manifests_else_the_listed_and_what_was_asked_since() {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(scopes_asked(&s(&["repo"]), &s(&["read", "write"]), &s(&["repo", "admin"])), s(&["repo", "admin"]));
    assert_eq!(scopes_asked(&[], &s(&["read"]), &[]), s(&["read"]));
}

/// RFC 9207: an answer from another server is refused, and so is one
/// naming no one from a server that says it names itself.
#[test]
fn an_answer_from_another_server_is_refused() {
    let issuer = "https://auth.example.com";
    assert!(check_answer_issuer(Some("https://auth.example.com/"), issuer, true).is_ok());
    assert!(check_answer_issuer(None, issuer, false).is_ok(), "a server that does not name itself");
    let e = check_answer_issuer(Some("https://evil.example"), issuer, false).unwrap_err().to_string();
    assert!(e.contains("answered by https://evil.example"), "{e}");
    assert!(check_answer_issuer(None, issuer, true).is_err(), "a missing iss from a server that sends one");
}

/// The verifier goes with the code only when a challenge went out, the
/// resource only when there is one, the secret only when the client has one.
#[test]
fn the_token_request_sends_the_verifier_only_with_pkce() {
    let keys = |f: &[(&'static str, String)]| f.iter().map(|(k, _)| *k).collect::<Vec<_>>();
    let p = waiting("gitlab", "mo", true, "https://mcp.example.com/mcp", Instant::now());
    let f = token_form("the-code", &p);
    assert_eq!(keys(&f), ["grant_type", "code", "redirect_uri", "client_id", "code_verifier", "resource"]);
    assert!(f.contains(&("code_verifier", "the-verifier".into())) && f.contains(&("code", "the-code".into())));
    let mut p = waiting("gitlab", "mo", false, "", Instant::now());
    p.client.client_secret = Some("s3cret".into());
    assert_eq!(keys(&token_form("c", &p)), ["grant_type", "code", "redirect_uri", "client_id", "client_secret"]);
}

/// A waiting sign-in is finished only by who started it, for that app,
/// once, and not after `PENDING_FOR`.
#[test]
fn only_its_person_finishes_a_waiting_sign_in() {
    let s = SignIns::default();
    s.wait("st".into(), waiting("gitlab", "mo", true, "", Instant::now()));
    assert!(s.take("st", "gitlab", "sam").is_none(), "another person took mo's sign-in");
    assert!(s.take("st", "slack", "mo").is_none(), "taken for another app");
    assert!(s.take("st", "gitlab", "mo").is_some(), "mo could not finish their own sign-in");
    assert!(s.take("st", "gitlab", "mo").is_none(), "a sign-in finished twice");
    let old = Instant::now().checked_sub(PENDING_FOR + Duration::from_secs(1)).unwrap();
    s.wait("old".into(), waiting("gitlab", "mo", true, "", old));
    assert!(s.take("old", "gitlab", "mo").is_none(), "an expired sign-in was finished");
    let mut slack = waiting("slack", "mo", true, "", Instant::now());
    slack.redirect_uri = SLACK_OAUTH_CALLBACK.into();
    s.wait("sl".into(), slack);
    assert!(s.slack_open("sl") && !s.slack_open("st"));
}

/// The account a sign-in names: its ID token's email, else Slack's person
/// and workspace.
#[test]
fn the_account_is_named_from_the_answer() {
    use base64::Engine;
    let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"email":"mo@example.com","name":"Mo"}"#);
    assert_eq!(account_of(&json!({ "id_token": format!("h.{claims}.s") })).as_deref(), Some("mo@example.com"));
    assert_eq!(account_of(&json!({ "authed_user": { "id": "U1" }, "team": { "name": "Veyra" } })).as_deref(), Some("U1 in Veyra"));
    assert_eq!(account_of(&json!({ "access_token": "x" })), None);
}

#[test]
fn fresh_tokens_expire_when_the_answer_says_and_are_not_lapsed() {
    let before = chrono::Utc::now().timestamp();
    let t = fresh_tokens(Grant { access: "a".into(), refresh: Some("r".into()), expires: Some(3600), account: None }, client("", None));
    assert!(t.expires_at.is_some_and(|at| at >= before + 3600 && at <= before + 3601 + 5), "{:?}", t.expires_at);
    assert_eq!((t.lifetime, t.lapsed, t.refresh_token.as_deref()), (Some(3600), false, Some("r")));
    assert!(t.connected_at.is_some_and(|c| c >= before) && t.wants.is_empty());
    let never = fresh_tokens(Grant { access: "a".into(), refresh: None, expires: None, account: None }, client("", None));
    assert_eq!((never.expires_at, never.lifetime), (None, None));
}
