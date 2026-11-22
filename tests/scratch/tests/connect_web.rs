//! Connecting an app from a web browser, in the real app
//! (`decisions/2026-11-21-connect-on-the-web.md`): the sign-in comes back to
//! the server's own `/connected`, where the app finishes it.
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;
fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_app_connects_from_a_web_browser() {
    let model = FakeModel::start(script(|_| async move { Reply::Text("Recorded.".into()) })).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let cal = FakeOAuthApp::start(3600).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private+untrusted" })).await;
    assert!(st.is_success(), "{v}");
    assert!(set_password(&app, "alice", PASSWORD));
    // The browser reaches the server at localhost and the app at 127.0.0.1:
    // the way back from the app's sign-in comes from another site.
    let url = app.base.replace("127.0.0.1", "localhost");
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/connect-web.mjs"))
        .env("MAGNUS_URL", &url)
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_CHROMIUM", chromium())
        .env("MAGNUS_APP", "calendar")
        .output()
        .await
        .unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    println!("{}", String::from_utf8_lossy(&out.stdout));
}
