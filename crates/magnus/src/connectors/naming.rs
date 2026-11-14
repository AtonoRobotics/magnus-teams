//! Naming and token-answer tests for `connectors`, kept apart from it for its size.
/// An app added by its address is named by what it calls itself, else
/// by the part of its host that says something, made unique.
#[test]
fn an_app_is_named_from_its_address() {
    let none = |_: &str| false;
    assert_eq!(super::name_from("https://mcp.notion.com/mcp", None, none), "notion");
    assert_eq!(super::name_from("https://mcp.linear.app/mcp", None, none), "linear");
    assert_eq!(super::name_from("https://drivemcp.googleapis.com/mcp/v1", None, none), "drivemcp");
    assert_eq!(super::name_from("https://mcp.example.com/mcp", Some("Team Calendar"), none), "team-calendar");
    assert_eq!(super::name_from("http://127.0.0.1:8080/mcp", None, none), "app");
    assert_eq!(super::name_from("https://mcp.notion.com/mcp", None, |n| n == "notion"), "notion-2");
}

/// A Slack answer that also carries a bot token keeps the person's
/// user token, and a dead refresh is told apart from a limit.
#[test]
fn a_slack_answer_keeps_the_persons_token_not_the_bots() {
    let both = serde_json::json!({
        "ok": true, "access_token": "xoxb-bot", "token_type": "bot", "refresh_token": "xoxe-bot", "expires_in": 43200,
        "authed_user": { "id": "U1", "access_token": "xoxp-user", "refresh_token": "xoxe-user", "expires_in": 43200, "token_type": "user" },
        "team": { "id": "T1", "name": "Veyra" }
    });
    let g = super::granted(&both).unwrap();
    assert_eq!((g.access.as_str(), g.refresh.as_deref(), g.expires, g.account.as_deref()), ("xoxp-user", Some("xoxe-user"), Some(43200), Some("U1 in Veyra")));
    let user = serde_json::json!({ "ok": true, "access_token": "xoxp-only", "token_type": "user", "refresh_token": "xoxe-1", "expires_in": 43200, "authed_user": { "id": "U2" }, "team": { "name": "Veyra" } });
    assert_eq!(super::granted(&user).unwrap().access, "xoxp-only");
    let google = serde_json::json!({ "access_token": "ya29", "refresh_token": "1//", "expires_in": 3600, "token_type": "Bearer" });
    assert_eq!(super::granted(&google).unwrap().access, "ya29");
    assert!(super::granted(&serde_json::json!({ "ok": false, "error": "invalid_code" })).is_err());
    let bounced = super::slack_handoff_url("st", Some("the-code"), Some("https://mcp.slack.com"), None);
    assert!(bounced.starts_with("http://127.0.0.1:17321/callback?"));
    let back: std::collections::HashMap<String, String> = url::Url::parse(&bounced).unwrap().query_pairs().into_owned().collect();
    assert_eq!(back.get("code").map(String::as_str), Some("the-code"));
    assert_eq!(back.get("state").map(String::as_str), Some("st"));
    assert_eq!(back.get("iss").map(String::as_str), Some("https://mcp.slack.com"));
    assert!(!bounced.contains("client_secret"));
    assert!(super::dead_token(&serde_json::json!({ "ok": false, "error": "invalid_refresh_token" })));
    assert!(!super::dead_token(&serde_json::json!({ "ok": false, "error": "ratelimited" })));
    assert!(!super::dead_token(&google));
}

/// A refused registration waits out the server's `Retry-After`, capped,
/// else a short backoff (#315).
#[test]
fn a_refused_registration_waits_out_retry_after_then_backs_off() {
    use std::time::Duration;
    let headers = |v: Option<&str>| {
        let mut h = reqwest::header::HeaderMap::new();
        if let Some(v) = v {
            h.insert("retry-after", v.parse().unwrap());
        }
        h
    };
    assert_eq!(super::registration_backoff(&headers(Some("3")), 0), Duration::from_secs(3));
    assert_eq!(super::registration_backoff(&headers(Some("120")), 0), Duration::from_secs(10));
    assert_eq!(super::registration_backoff(&headers(None), 0), Duration::from_secs(1));
    assert_eq!(super::registration_backoff(&headers(None), 1), Duration::from_secs(2));
    assert_eq!(super::registration_backoff(&headers(Some("tomorrow")), 1), Duration::from_secs(2));
}
