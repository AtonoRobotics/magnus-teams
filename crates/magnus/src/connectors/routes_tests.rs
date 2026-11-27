//! The apps' route bodies are of a fixed shape: one that does not fit is
//! refused with 422 and why, never read as empty.

use crate::admin_app::tests::server_with;
use axum::Router;
use reqwest::Method;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn a_body_that_does_not_fit_is_refused_not_read_as_empty() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    crate::admin_app::guard::mark(&sam, chrono::Utc::now());
    for (why, body) in [
        ("no address", json!({ "audience": "private", "label": "private" })),
        ("an address that is not text", json!({ "url": 5, "audience": "private", "label": "private" })),
        ("groups that are not a list", json!({ "url": "https://mcp.example.com/mcp", "audience": "private", "label": "private", "access_groups": "staff" })),
        ("a group that is not a name", json!({ "url": "https://mcp.example.com/mcp", "audience": "private", "label": "private", "blocked_groups": [7] })),
        ("a record flag that is not true or false", json!({ "url": "https://mcp.example.com/mcp", "audience": "private", "label": "private", "system_of_record": "yes" })),
    ] {
        let (st, v) = s.call(&sam, Method::POST, "/v1/connectors", Some(body)).await;
        assert_eq!(st, 422, "{why}: {v}");
        assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()), "{why}: no reason given: {v}");
    }
    // A risk that is not text was read as none: the tool would no longer be offered.
    let (st, v) = s.call(&sam, Method::PUT, "/v1/connectors/calendar/tools/list", Some(json!({ "risk": 5 }))).await;
    assert_eq!(st, 422, "{v}");
    let (st, _) = s.call(&sam, Method::PUT, "/v1/connectors/calendar/tools/list", Some(json!({ "risk": null }))).await;
    assert_eq!(st, 404, "a null risk fits, and the app is unknown");
    s.done().await;
}

/// An owner's Apps asks every app for its tools at once. One after another,
/// a few slow apps kept Apps blank for seconds (owner, 2026-11-27: "Apps
/// opens with a very long delay"). Each fake answers only once all three
/// are being asked, so the old way never answers.
#[tokio::test(flavor = "multi_thread")]
async fn an_owners_apps_asks_every_app_at_once() {
    use crate::transport::fake::Fake;
    const APPS: usize = 3;
    const WAIT_SECONDS: u64 = 20;
    let all = std::sync::Arc::new(tokio::sync::Barrier::new(APPS));
    let mut config = String::new();
    for n in 0..APPS {
        let (url, _) = Fake { listing: Some(all.clone()), ..Fake::new(crate::transport::Kind::Brokered) }.install();
        config += &format!("[[connectors]]\nname = \"app{n}\"\nurl = \"{url}\"\naudience = \"internal\"\nlabel = \"private\"\n");
    }
    let s = server_with(&config, |_| Router::new()).await;
    let sam = s.token("sam").await;
    let listed = tokio::time::timeout(std::time::Duration::from_secs(WAIT_SECONDS), s.call(&sam, Method::GET, "/v1/connectors", None)).await;
    let (st, v) = listed.expect("the apps were asked one after another");
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        v["connectors"].as_array().map(|c| c.iter().filter(|c| c["name"].as_str().is_some_and(|n| n.starts_with("app")) && c["problem"].is_null()).count()),
        Some(APPS),
        "{v}"
    );
    s.done().await;
}
