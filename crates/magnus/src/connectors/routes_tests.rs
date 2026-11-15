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
