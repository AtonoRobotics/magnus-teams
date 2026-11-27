//! A table in a chat answer is drawn as tall as its rows' text (owner,
//! 2026-11-27: a two-column Priority / Finding table had hundreds of pixels
//! of blank space above and below its one row). The chat answers with prose
//! and a `magnus-ui` table whose cells hold long links, as a review does;
//! `desktop/tests/chat-table.mjs` opens the conversation in Nova at the
//! owner's window size and measures the rows.
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

const ANSWER: &str = "Memory is close. Personal facts load into personal runs only; team runs and project channels get no personal facts. Keep those boundaries and the single Postgres store.

```magnus-ui
table columns=Priority,Finding,Fix
  row \"P1 · Replace stale facts explicitly\",\"The [duplicate check](https://github.com/AtonoRobotics/magnus-teams/blob/4d82f95b9604b988f7b5c711b535e9bc1f945a25/crates/magnus/src/memory/store.rs#L210) uses a 0.85 word-overlap threshold. Adding “never” to “The person wants weekly plan email” reverses the meaning but scores 10/11 (0.909). Existing tests cover re-statements, and the [supersession event definition](https://github.com/AtonoRobotics/magnus-teams/blob/4d82f95b9604b988f7b5c711b535e9bc1f945a25/crates/magnus/src/events/memory.rs#L88) exists, but nothing currently writes that event.\",\"Write the supersession event when a fact is corrected, and compare negations before calling two facts the same.\"
  row \"P2 · Show what was remembered\",\"A person cannot see which facts a run loaded.\",\"List the loaded facts in the run's details.\"
```

Both fixes are small.";

#[tokio::test(flavor = "multi_thread")]
async fn a_chat_answers_table_is_as_tall_as_its_rows() {
    let model = FakeModel::start(script(|_| async move { Reply::Text(ANSWER.into()) })).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let (status, chat) = alice.post("/v1/chat", json!({ "text": "Review how memory is going." })).await;
    assert!(status.is_success(), "{chat}");
    let thread = chat["thread"].as_str().unwrap().to_string();
    eventually("the answer with its table", PATIENCE, || async {
        let (_, c) = alice.get(&format!("/v1/chat/{thread}")).await;
        c["entries"].as_array()?.iter().any(|e| e["text"].as_str().is_some_and(|t| t.contains("magnus-ui"))).then_some(())
    })
    .await;
    assert!(set_password(&app, "alice", PASSWORD));
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/chat-table.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_THREAD", &thread)
        .env("MAGNUS_CHROMIUM", chromium())
        .envs(shots())
        .output()
        .await
        .unwrap();
    let log = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    println!("{log}");
    assert!(out.status.success(), "the chat's table is not drawn to its rows:\n{log}");
}
