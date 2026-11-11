//! Pop-ups, deleting and archiving, membership and deleting a workspace, in
//! a real browser against a real server (desktop/tests/team-own-app.mjs).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn messages_pop_up_and_people_delete_archive_and_leave() {
    let model = FakeModel::start(script(|_| async { complete("done") })).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let (code, made) = alice.post("/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(code, 200, "{made}");
    let ws = made["id"].as_str().unwrap().to_string();
    let (code, v) = alice.post(&format!("/v1/workspaces/{ws}/members"), json!({ "person": "bob", "role": "member" })).await;
    assert_eq!(code, 200, "{v}");
    let (code, v) = alice.post(&format!("/v1/workspaces/{ws}/direct"), json!({ "people": ["bob"] })).await;
    assert_eq!(code, 200, "{v}");
    let dm = v["id"].as_str().unwrap().to_string();
    for who in ["alice", "bob"] {
        assert!(set_password(&app, who, PASSWORD));
    }
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/team-own-app.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_WS", &ws)
        .env("MAGNUS_DM", &dm)
        .env("MAGNUS_CHROMIUM", chromium())
        .envs(shots())
        .output()
        .await
        .unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}
