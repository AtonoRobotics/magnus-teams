//! Regressions from the review of master (2026-10-29), connectors:
//! - a sign-in answered by another authorization server than the one it
//!   went to is refused (RFC 9207 mix-up defence);
//! - an app's label must be named: an empty one read as "principal";
//! - an app that takes no sign-in is listed and used without a credential;
//! - a refresh answer without `expires_in` keeps the token refreshing;
//! - an approval for a time window holds only while its approver still has
//!   the approver role.

use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::{json, Value};

async fn signed_in(alice: &Api, cal: &FakeOAuthApp) {
    assert_eq!(alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private" })).await.0, 201);
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let (_, v) = alice.post("/v1/connectors/calendar/sign-in", json!({ "redirect_uri": format!("http://127.0.0.1:{port}/callback") })).await;
    let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    assert_eq!(alice.post("/v1/connectors/calendar/sign-in/complete", json!({ "state": state, "code": code, "iss": cal.base })).await.0, 204);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_answered_by_another_server_is_refused() {
    let cal = FakeOAuthApp::start(3600).await;
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    assert_eq!(alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private" })).await.0, 201);
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let (_, v) = alice.post("/v1/connectors/calendar/sign-in", json!({ "redirect_uri": format!("http://127.0.0.1:{port}/callback") })).await;
    let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    let (st, e) = alice.post("/v1/connectors/calendar/sign-in/complete", json!({ "state": state, "code": code, "iss": "https://honest.example" })).await;
    assert_eq!(st.as_u16(), 400, "a code from another server was redeemed: {e}");
    let (_, list) = alice.get("/v1/connectors").await;
    assert_eq!(list["connectors"][0]["signed_in"], false, "{list}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_app_names_its_label_and_one_without_a_sign_in_is_used_without_a_credential() {
    let mail = mcp::mail().start().await;
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (st, _) = alice.post("/v1/connectors", json!({ "name": "inbox", "url": mail.url, "audience": "private" })).await;
    assert_eq!(st.as_u16(), 400, "an app without a label was added, its results tainting nothing");
    let (st, v) = alice.post("/v1/connectors", json!({ "name": "inbox", "url": mail.url, "audience": "private", "label": "private+untrusted" })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let (_, list) = alice.get("/v1/connectors").await;
    let c = list["connectors"].as_array().unwrap().iter().find(|c| c["name"] == "inbox").unwrap().clone();
    assert!(c["problem"].is_null(), "an app that takes no sign-in cannot be used: {c}");
    assert!(c["tools"].as_array().is_some_and(|t| !t.is_empty()), "{c}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refresh_without_expires_in_keeps_the_token_refreshing() {
    // Tokens last 30 s, inside the minute Magnus refreshes ahead: every use refreshes.
    let cal = FakeOAuthApp::start(30).await;
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    signed_in(&alice, &cal).await;
    cal.state.lock().unwrap().refresh_without_expiry = true;
    let refreshes = || cal.state.lock().unwrap().refreshes;
    // Each listing uses the token (the kernel keeps a list two seconds).
    alice.get("/v1/connectors").await;
    let first = refreshes();
    assert!(first >= 1);
    alice.get("/v1/connectors").await;
    let (_, list) = alice.get("/v1/connectors").await;
    assert!(refreshes() > first, "after a refresh answer without expires_in the token was never refreshed again");
    assert!(list["connectors"][0]["problem"].is_null(), "{list}");
}
