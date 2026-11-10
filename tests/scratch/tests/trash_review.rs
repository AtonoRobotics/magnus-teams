//! Regressions from the review of master's trash and voice (2026-10-30):
//! - a removed task takes its verdict's note and the person's answers to its
//!   cards with it, and leaves no copy in the backups kept;
//! - a removed memory takes the call and reply that wrote it with it;
//! - a restore racing a removal never brings back an item with no content;
//! - read-aloud has a length, a number of pieces at once, and a time.

use magnus_invariants::appliance::bin;
use magnus_invariants::model::Script;
use magnus_invariants::*;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;

const CHAT: &str = "talking with the person you work for in a chat";
const PASS: &str = "You keep the person's long-term memory";
const FACT: &str = "The person's code word is CODEWORD-CANARY-51d.";

fn model() -> Script {
    script(|c| async move {
        let system = c.body["system"].to_string();
        if system.contains(PASS) {
            return Reply::Text(String::new());
        }
        if system.contains(CHAT) {
            return match c.turn() {
                0 => tool("memory_write", json!({ "fact": FACT })),
                _ => Reply::Text("Noted.".into()),
            };
        }
        match c.turn() {
            0 => tool("ask_principal", json!({ "question": "Which tone?" })),
            1 => contract_unchecked("write it", "it is written"),
            _ => complete("written"),
        }
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_task_takes_its_notes_and_answers_and_leaves_the_backups() {
    let m = FakeModel::start(model()).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let t = alice.submit("Write about TASK-CANARY-3e1.").await;
    let q = alice.wait_item(&t, "question").await;
    assert_eq!(alice.respond(q["id"].as_str().unwrap(), json!({ "decision": "answer", "text": "ANSWER-CANARY-77a" })).await.as_u16(), 200);
    alice.wait_settled(&t).await;
    let (st, v) = alice.post(&format!("/v1/tasks/{t}/verdict"), json!({ "verdict": "partly", "note": "NOTE-CANARY-9b2" })).await;
    assert!(st.is_success(), "{v}");
    // A backup with all of it, made before the task is deleted.
    let (st, b) = alice.post("/v1/backups", json!({})).await;
    assert_eq!(st, 201, "{b}");
    let file = app.state_dir.join("backups").join(b["file"].as_str().unwrap());

    assert!(alice.post("/v1/trash", json!({ "item": t })).await.0.is_success());
    assert!(alice.send(reqwest::Method::DELETE, &format!("/v1/trash/{t}"), None).await.0.is_success());
    let dump = app.db.dump().await;
    for canary in ["TASK-CANARY-3e1", "ANSWER-CANARY-77a", "NOTE-CANARY-9b2"] {
        assert!(!dump.contains(canary), "{canary} is still stored after the task was removed");
    }

    // The backup made before still restores, without the removed content.
    app.kill().await;
    app.db.admin().await.batch_execute("DROP TABLE public.content, public.events").await.unwrap();
    std::fs::remove_dir_all(app.state_dir.join("vault")).unwrap();
    let out = std::process::Command::new(bin("magnus-appliance"))
        .args(["restore", "--config"])
        .arg(app.dir.path().join("magnus.toml"))
        .args(["--admin-url", &app.db.admin_url(), "--from"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success(), "restore failed: {}", String::from_utf8_lossy(&out.stderr));
    let dump = app.db.dump().await;
    for canary in ["TASK-CANARY-3e1", "ANSWER-CANARY-77a", "NOTE-CANARY-9b2"] {
        assert!(!dump.contains(canary), "{canary} came back from a backup made before the removal");
    }
    app.restart().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_memory_takes_where_it_was_written_with_it() {
    let m = FakeModel::start(model()).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/chat", json!({ "text": "Please keep my code word." })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    alice.wait_settled(v["task_id"].as_str().unwrap()).await;
    let mem = eventually("the memory", PATIENCE, || async {
        alice.get("/v1/memory").await.1["memory"].as_array().and_then(|a| a.first()).and_then(|m| m["id"].as_str()).map(String::from)
    })
    .await;
    assert!(app.db.dump().await.contains("CODEWORD-CANARY-51d"));
    assert!(alice.send(reqwest::Method::DELETE, &format!("/v1/memory/{mem}"), None).await.0.is_success());
    assert!(alice.send(reqwest::Method::DELETE, &format!("/v1/trash/{mem}"), None).await.0.is_success());
    let dump = app.db.dump().await;
    assert!(!dump.contains("CODEWORD-CANARY-51d"), "the removed memory is still stored where it was written");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restore_racing_a_removal_never_brings_back_an_empty_item() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    for i in 0..6 {
        let words = format!("RACE-CANARY-{i}");
        let t = alice.submit(&format!("Say {words}.")).await;
        alice.wait_settled(&t).await;
        assert!(alice.post("/v1/trash", json!({ "item": t })).await.0.is_success());
        let (a, b) = (app.api("alice"), app.api("alice"));
        let (t1, t2) = (t.clone(), t.clone());
        let restore = tokio::spawn(async move { a.post(&format!("/v1/trash/{t1}/restore"), json!({})).await.0 });
        let remove = tokio::spawn(async move { b.send(reqwest::Method::DELETE, &format!("/v1/trash/{t2}"), None).await.0 });
        let (_, _) = (restore.await.unwrap(), remove.await.unwrap());
        let (st, task) = alice.get(&format!("/v1/tasks/{t}")).await;
        if st.is_success() {
            assert_eq!(task["text"].as_str(), Some(format!("Say {words}.").as_str()), "a restored task came back without its content: {task}");
        }
    }
}

/// A stand-in for Kokoro that takes `secs` to speak.
fn slow_speak(dir: &std::path::Path, secs: u32) -> std::path::PathBuf {
    let prog = dir.join(format!("speak-{secs}"));
    std::fs::write(&prog, format!("#!/bin/sh\ncat > /dev/null\nsleep {secs}\nprintf 'ID3'\n")).unwrap();
    std::fs::set_permissions(&prog, std::fs::Permissions::from_mode(0o755)).unwrap();
    prog
}

#[tokio::test(flavor = "multi_thread")]
async fn read_aloud_has_a_length_a_number_at_once_and_a_time() {
    let m = FakeModel::start(steps(vec![])).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let prog = slow_speak(app.dir.path(), 2);
    magnus_scratch::reconfigure(&mut app, |c| {
        let mut r = toml::Table::new();
        r.insert("speak".into(), prog.display().to_string().into());
        r.insert("model".into(), "/voice/kokoro.onnx".into());
        r.insert("voices".into(), "/voice/voices.bin".into());
        r.insert("voice".into(), "af_heart".into());
        r.insert("timeout_s".into(), 1.into());
        c.insert("read_aloud".into(), r.into());
    })
    .await;
    let alice = app.api("alice");
    assert_eq!(alice.post("/v1/speak", json!({ "text": "word ".repeat(1000) })).await.0, 413, "a whole book was read aloud at once");
    let start = std::time::Instant::now();
    let (st, _) = alice.post("/v1/speak", json!({ "text": "Hello." })).await;
    assert_eq!(st, 504, "a piece that takes too long was not stopped");
    assert!(start.elapsed() < std::time::Duration::from_millis(1900), "it was not stopped in time");
    // Three at once: the voice makes two, the third is told to wait.
    let codes: Vec<u16> = futures::future::join_all((0..3).map(|_| {
        let a = app.api("alice");
        async move { a.post("/v1/speak", json!({ "text": "Hello." })).await.0.as_u16() }
    }))
    .await;
    assert!(codes.contains(&429), "every piece was made at once: {codes:?}");
    let _: Value = json!(codes);
}
