//! Aurora on a phone, in the real app with a real computer
//! (`decisions/2026-11-21-aurora-phone.md`, `desktop/tests/aurora-phone.mjs`).
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::{json, Value};

/// How long the server has to make alice's computer.
const MADE_WITHIN: std::time::Duration = std::time::Duration::from_secs(60);

#[tokio::test(flavor = "multi_thread")]
async fn aurora_on_a_phone_keeps_the_computer_on_the_screen() {
    let model = FakeModel::start(script(|_| async move { Reply::Text("Recorded.".into()) })).await;
    let computerd = Computerd::start();
    let app = Appliance::start(Spec::default().computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let made: Value = eventually("the server made alice's computer", MADE_WITHIN, || async {
        let (_, v) = alice.get("/v1/computers").await;
        v["computers"].as_array().into_iter().flatten().find(|c| c["owner"]["person"] == "alice").cloned()
    })
    .await;
    let id = made["id"].as_str().unwrap_or_default().to_string();
    let (st, v) = alice.post(&format!("/v1/computers/{id}/start"), json!({})).await;
    assert!(st.is_success(), "start: {v}");
    let (st, v) = alice.post("/v1/chat", json!({ "text": "Draft the budget note" })).await;
    assert!(st.is_success(), "{v}");
    assert!(set_password(&app, "alice", PASSWORD));
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = tokio::process::Command::new("node")
        .arg(root.join("desktop/tests/aurora-phone.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_CHROMIUM", chromium())
        .env("MAGNUS_COMPUTER", &id)
        .envs(shots())
        .output()
        .await
        .unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    println!("{}", String::from_utf8_lossy(&out.stdout));
}
