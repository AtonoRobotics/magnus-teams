//! Regression from the review of master (2026-10-29): desktop 0.4.5 names
//! no version, so the appliance's "update required" never reached it. An
//! app that signs in without naming its version is told to update.

use magnus_invariants::*;
use magnus_scratch::set_password;
use serde_json::{json, Value};

#[tokio::test(flavor = "multi_thread")]
async fn an_app_that_names_no_version_is_told_to_update_at_sign_in() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    assert!(set_password(&app, "alice", "correct horse battery"));
    let http = reqwest::Client::new();
    let sign_in = |version: Option<&str>, client: &str| {
        let mut r = http.post(format!("{}/v1/sign-in", app.base)).json(&json!({ "person": "alice", "password": "correct horse battery", "client": client }));
        if let Some(v) = version {
            r = r.header("x-magnus-app", v);
        }
        r.send()
    };
    // 0.4.5, as it signs in.
    let r = sign_in(None, "app").await.unwrap();
    assert_eq!(r.status(), 426);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["update_required"], true, "{v}");
    // A current app, and the browser page, sign in.
    assert!(sign_in(Some("0.5.0"), "app").await.unwrap().status().is_success());
    assert!(sign_in(None, "browser").await.unwrap().status().is_success());
}
