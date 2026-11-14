//! Reference viewport and keyboard behavior through the real browser bridge.
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;
fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}
#[tokio::test(flavor = "multi_thread")]
async fn partial_refresh_failure_retains_cached_data_and_reconciles_latest() {
    let mut model = FakeModel::start(script(|c| async move {
        if c.body["system"].to_string().contains("talking with the person you work for in a chat") {
            return Reply::Text("Recorded.".into());
        }
        match c.turn() {
            0 => plan("Readable result"),
            _ => tool("task_complete", json!({"summary":"Verification result","deliverable":[{"type":"text","text":"Actual stored deliverable"}]})),
        }
    }))
    .await;
    model.url = provider(&model).await;
    let mail = magnus_invariants::mcp::mail().start().await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible")]), &model).await;
    let alice = app.api("alice");
    let task = alice.submit("Verify work details.").await;
    alice.wait_status(&task, &["completed"]).await;
    assert!(set_password(&app, "alice", PASSWORD));
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/workspace-status.mjs"))
        .env("MAGNUS_RESULT_TASK", &task)
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
