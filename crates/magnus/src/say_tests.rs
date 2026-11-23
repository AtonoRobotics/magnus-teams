//! One request shape for saying something: a shared conversation's message
//! and a whisper keep the reasoning picked, as the personal chat does, and
//! a whisper takes the person's own files to their own Magnus while a
//! message to the channel takes none.

use crate::admin_app::tests::{server_with, Server};
use axum::Router;
use serde_json::{json, Value};

/// A shared project of sam's, and its first channel's messages path.
async fn launch(s: &Server, sam: &str) -> (String, String, String) {
    let (_, v) = s.post(sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    let ws = v["id"].as_str().unwrap().to_string();
    let channel = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
    let path = format!("/v1/workspaces/{ws}/channels/{channel}/messages");
    (ws, channel, path)
}

fn picked(s: &Server, reasoning: &str) -> Value {
    let (adapter, model) = s.k.default_model();
    json!({ "adapter": adapter, "model": model, "reasoning": reasoning })
}

fn run_of(s: &Server, v: &Value) -> crate::state::Run {
    let id = v["run_id"].as_str().unwrap_or_else(|| panic!("no run started: {v}"));
    s.k.read(|st| st.runs.get(id).cloned()).unwrap()
}

/// The reasoning a member picks reaches their own agent's run, as it does
/// from their personal chat; a value that is not an effort is refused.
#[tokio::test(flavor = "multi_thread")]
async fn a_shared_request_runs_on_the_reasoning_picked() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let (_, _, path) = launch(&s, &sam).await;
    let (st, v) = s.post(&sam, &path, json!({ "text": "plan it", "agent": true, "model": picked(&s, "high") })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(run_of(&s, &v).config.reasoning.as_deref(), Some("high"), "the shared request dropped the reasoning picked");
    let (st, v) = s.post(&sam, &path, json!({ "text": "plan it", "agent": true, "model": picked(&s, "loud") })).await;
    assert_eq!(st, 400, "a reasoning that is no effort was taken: {v}");
    s.done().await;
}

/// A whisper takes the person's own file and the reasoning picked to their
/// own Magnus; the file never reaches the shared Files.
#[tokio::test(flavor = "multi_thread")]
async fn a_whisper_takes_its_files_and_reasoning_to_ones_own_magnus() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let (ws, _, path) = launch(&s, &sam).await;
    let (st, f) = s.post(&sam, "/v1/files", json!({ "name": "payslip.txt", "media_type": "text/plain", "data_b64": "aGVsbG8=" })).await;
    assert_eq!(st, 201, "{f}");
    let file = f["id"].as_str().unwrap().to_string();
    let (st, v) = s.post(&sam, &path, json!({ "text": "what do I owe?", "whisper": true, "files": [file], "model": picked(&s, "low") })).await;
    assert_eq!(st, 200, "{v}");
    let run = run_of(&s, &v);
    assert_eq!(run.config.reasoning.as_deref(), Some("low"), "the whisper dropped the reasoning picked");
    let task = v["task_id"].as_str().unwrap();
    assert_eq!(s.k.read(|st| st.tasks.get(task).map(|t| t.files.clone())), Some(vec![file.clone()]), "the whisper dropped its file");
    assert!(!s.k.read(|st| st.team.workspace(&ws).unwrap().files.contains_key("payslip.txt")), "a whisper's file reached the shared Files");
    s.done().await;
}

/// Only the person's own uploads go with a whisper, and a message to the
/// channel carries none: its files are the shared Files it names.
#[tokio::test(flavor = "multi_thread")]
async fn only_a_whisper_takes_files_and_only_ones_own() {
    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    let ana = s.token("ana").await;
    let (ws, channel, path) = launch(&s, &sam).await;
    let (_, f) = s.post(&ana, "/v1/files", json!({ "name": "hers.txt", "media_type": "text/plain", "data_b64": "aGVsbG8=" })).await;
    let hers = f["id"].as_str().unwrap().to_string();
    let (st, v) = s.post(&sam, &path, json!({ "text": "read this", "whisper": true, "files": [hers] })).await;
    assert_eq!(st, 404, "a whisper took another person's file: {v}");
    let (_, f) = s.post(&sam, "/v1/files", json!({ "name": "mine.txt", "media_type": "text/plain", "data_b64": "aGVsbG8=" })).await;
    let mine = f["id"].as_str().unwrap().to_string();
    let (st, v) = s.post(&sam, &path, json!({ "text": "read this", "agent": true, "files": [mine] })).await;
    assert_eq!(st, 400, "a message to the channel took files of one's own: {v}");
    assert!(s.k.read(|st| st.team.workspace(&ws).unwrap().channels[&channel].messages.is_empty()), "the refused message was posted");
    s.done().await;
}
