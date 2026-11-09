//! Unsent messages kept for the session (#544) and reading anchors, through
//! the real appliance and browser.
//! Fixed replies exercise UI behavior, not agent reasoning.
use magnus_invariants::*;
fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}
use magnus_scratch::*;

#[test]
fn drafts_live_in_memory_one_per_conversation_and_never_on_the_device() {
    let out = std::process::Command::new("node").arg(workspace_root().join("desktop/tests/draft-check.mjs")).output().unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}

#[tokio::test(flavor = "multi_thread")]
async fn drafts_uploads_and_uncertain_sends_survive_navigation_without_resending() {
    let m = FakeModel::start(script(|_| async { Reply::Text("Recorded.".into()) })).await;
    let app = Appliance::start(Spec::default(), &m).await;
    assert!(set_password(&app, "alice", PASSWORD));
    assert!(set_password(&app, "bob", PASSWORD));
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/workspace-continuity.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PERSON", "alice")
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_CHROMIUM", chromium())
        .envs(shots())
        .output()
        .await
        .unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    println!("{}", String::from_utf8_lossy(&out.stdout));
}
