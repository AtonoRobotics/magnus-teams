//! The personal chat and a team channel are one chat view (owner,
//! 2026-11-13: "Teams is a multi user version of the single chat
//! interface"): in a real browser against a real server, the same request in
//! each is drawn with the same message, answer, composer and styles, and
//! screenshots of both are kept to compare (desktop/tests/chat-looks-alike.mjs).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use magnus_invariants::model::Reply;
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;
use std::time::Duration;

/// How long the model takes, so a request is seen while it works.
const THINKING: Duration = Duration::from_millis(2500);

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_personal_chat_and_a_team_channel_draw_alike() {
    let model = FakeModel::start(script(|_| async {
        tokio::time::sleep(THINKING).await;
        Reply::Text("Here is the plan: **three** steps.\n\n1. Draft\n2. Review\n3. Ship".into())
    }))
    .await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let (code, made) = alice.post("/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(code, 200, "{made}");
    let ws = made["id"].as_str().unwrap().to_string();
    let (code, v) = alice.post(&format!("/v1/workspaces/{ws}/members"), json!({ "person": "bob", "role": "member" })).await;
    assert_eq!(code, 200, "{v}");
    assert!(set_password(&app, "alice", PASSWORD));
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/chat-looks-alike.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_WS", &ws)
        .env("MAGNUS_CHROMIUM", chromium())
        .envs(shots())
        .output()
        .await
        .unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}
