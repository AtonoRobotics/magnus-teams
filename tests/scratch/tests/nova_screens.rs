//! Nova's screens in the real app, framed as its mockup draws them
//! (`desktop/tests/nova-screens.mjs`).
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;
fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn nova_pages_have_the_mockups_top_bar() {
    let model = FakeModel::start(script(|_| async move { Reply::Text("Recorded.".into()) })).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    for text in ["**Plan** the offsite", "Draft the budget note", "Check the venue list"] {
        let (status, chat) = alice.post("/v1/chat", json!({ "text": text })).await;
        assert!(status.is_success(), "{chat}");
    }
    assert!(set_password(&app, "alice", PASSWORD));
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/nova-screens.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_CHROMIUM", chromium())
        .envs(shots())
        .output()
        .await
        .unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    println!("{}", String::from_utf8_lossy(&out.stdout));
}
