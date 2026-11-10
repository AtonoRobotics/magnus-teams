//! Regressions from the review of master (2026-10-29), the library and the
//! owner's views:
//! - a file whose name holds a control character or non-ASCII letters
//!   downloads, and is never sniffed as something to run;
//! - the owner gives verdicts on an experiment's trials, which run as
//!   other people;
//! - the owner sees another person's tasks, not what they said in chat or
//!   their projects.

use base64::Engine;
use magnus_invariants::appliance::Person;
use magnus_invariants::*;
use serde_json::{json, Value};

const CHAT: &str = "talking with the person you work for in a chat";

#[tokio::test(flavor = "multi_thread")]
async fn any_file_name_downloads() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let http = reqwest::Client::new();
    for name in ["Résumé 2026.txt", "two\nlines.txt", "q\"uote.txt"] {
        let (st, v) =
            alice.post("/v1/files", json!({ "name": name, "media_type": "text/plain\r\nx-evil: 1", "data_b64": base64::engine::general_purpose::STANDARD.encode("hello") })).await;
        if !st.is_success() {
            continue; // refused at upload is fine too
        }
        let id = v["id"].as_str().unwrap();
        let r = http.get(format!("{}/v1/files/{id}", app.base)).bearer_auth(&alice.token).send().await.expect("the download answered");
        assert_eq!(r.status(), 200, "{name:?} cannot be downloaded");
        assert_eq!(r.headers()["x-content-type-options"], "nosniff");
        assert!(r.headers().get("x-evil").is_none(), "a header was smuggled in through the type");
        assert!(r.headers()["content-disposition"].to_str().unwrap().starts_with("attachment;"));
        assert_eq!(r.text().await.unwrap(), "hello");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_owner_judges_trials_and_sees_others_tasks_but_not_their_chats() {
    let m = FakeModel::start(script(|c| async move {
        if c.body["system"].to_string().contains(CHAT) {
            return match c.turn() {
                0 => tool("create_task", json!({ "title": "Find a lawyer", "goal": "Find a divorce lawyer near Lisbon." })),
                _ => Reply::Text("Started.".into()),
            };
        }
        tool("task_fail", json!({ "reason": "no" }))
    }))
    .await;
    let mut s = Spec::default();
    s.people.push(Person { id: "dana".into(), name: "Dana".into(), token: "tok-dana-library-review".into(), groups: vec!["staff".into()], roles: vec!["developer".into()] });
    let app = Appliance::start(s, &m).await;
    let (alice, dana, carol) = (app.api("alice"), app.api("dana"), app.api("carol"));

    // A trial runs as dana; the owner gives its verdict.
    let (st, v) = dana.post("/v1/dev/experiments/rev/trials", json!({ "case": "c", "trial": 0, "text": "Say hello.", "modules": {} })).await;
    assert_eq!(st, 201, "{v}");
    let trial = v["task_id"].as_str().unwrap().to_string();
    dana.wait_settled(&trial).await;
    let (st, v) = alice.post(&format!("/v1/tasks/{trial}/verdict"), json!({ "verdict": "wrong" })).await;
    assert!(st.is_success(), "the owner could not judge a trial: {v}");
    let (_, judged) = dana.get(&format!("/v1/tasks/{trial}")).await;
    assert_eq!(judged["verdict"]["verdict"], "wrong");
    // Another person's own task stays theirs to judge (and theirs alone to see).
    let own = carol.submit("Say hello.").await;
    carol.wait_settled(&own).await;
    assert_eq!(alice.post(&format!("/v1/tasks/{own}/verdict"), json!({ "verdict": "wrong" })).await.0, 404);

    // Carol's chat starts a task; nothing of it, the sentence she wrote
    // least of all, reaches the owner's task list.
    let said = "I need a divorce lawyer, quietly please.";
    let (_, v) = carol.post("/v1/chat", json!({ "text": said })).await;
    carol.wait_settled(v["task_id"].as_str().unwrap()).await;
    let mine: Value = eventually("the task from chat", PATIENCE, || async {
        let t = carol.tasks().await;
        t["tasks"].as_array().unwrap().iter().any(|t| t["title"] == "Find a lawyer").then_some(t)
    })
    .await;
    assert!(mine.to_string().contains("quietly please"), "the person no longer sees their own conversation: {mine}");
    let tasks = alice.tasks().await;
    assert!(!tasks.to_string().contains("quietly please"), "the owner reads another person's chat: {tasks}");
}
