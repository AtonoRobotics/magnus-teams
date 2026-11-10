//! Reference viewport and keyboard behavior through the real browser bridge.
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;
fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}
#[tokio::test(flavor = "multi_thread")]
async fn responsive_workspace_preserves_access_and_preferences() {
    let model = FakeModel::start(script(|c| async move {
        if c.body["system"].to_string().contains("in a chat") {
            return Reply::Text("Recorded.".into());
        }
        match c.turn() {
            0 => tool("ask_principal", json!({"question":"Which section first?"})),
            1 => contract_unchecked("Navigation review", "Useful result"),
            _ => tool("task_complete", json!({"summary":"Navigation review completed","deliverable":[{"type":"text","text":"Actual reviewed navigation result"}]})),
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let task = alice.submit("Review navigation fixture").await;
    let question = alice.wait_item(&task, "question").await;
    let bob = app.api("bob");
    let (status, chat) = bob.post("/v1/chat", json!({"text":"Shared activity update"})).await;
    assert!(status.is_success(), "{chat}");
    let thread = chat["thread"].as_str().unwrap();
    let (_, entries) = bob.get(&format!("/v1/chat/{thread}")).await;
    let entry = entries["entries"].as_array().unwrap().iter().find(|e| e["kind"] == "message").unwrap()["id"].as_str().unwrap();
    let (status, shared) = bob.post(&format!("/v1/chat/{thread}/entries/{entry}/share"), json!({"person":"alice"})).await;
    assert!(status.is_success(), "{shared}");
    assert!(set_password(&app, "alice", PASSWORD));
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/workspace-layout.mjs"))
        .env("MAGNUS_QUESTION_TASK", &task)
        .env("MAGNUS_QUESTION_ITEM", question["id"].as_str().unwrap())
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

#[tokio::test(flavor = "multi_thread")]
async fn activity_updates_record_only_presented_items_and_retry_failures() {
    let model = FakeModel::start(script(|c| async move {
        if c.body["system"].to_string().contains("reviewing one person's state") && c.turn() == 0 {
            return tool("observe", json!({"what":"Actual Activity observation","why":"Useful update","evidence":"Recorded fixture source"}));
        }
        tool("task_complete", json!({"summary":"Review complete","deliverable":[{"type":"text","text":"Reviewed actual state"}]}))
    }))
    .await;
    // The harness installs the provider key after its first startup. Enable
    // reviews only after that credential exists, so their auth cannot race it.
    let mut app = Appliance::start(Spec::default(), &model).await;
    reconfigure(&mut app, |cfg| cfg["proactive"]["on"] = true.into()).await;
    // Keep the three-minute shared-runner budget from #512.
    let alice = app.api("alice");
    let observation = eventually("actual Alice observation", std::time::Duration::from_secs(180), || {
        let api = alice.clone();
        async move { api.inbox().await.into_iter().find(|i| i["kind"] == "observation" && i["status"] == "open") }
    })
    .await;
    assert!(set_password(&app, "alice", PASSWORD));
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/workspace-opened.mjs"))
        .env("MAGNUS_OBSERVATION", observation["id"].as_str().unwrap())
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_CHROMIUM", chromium())
        .output()
        .await
        .unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    println!("{}", String::from_utf8_lossy(&out.stdout));
}
