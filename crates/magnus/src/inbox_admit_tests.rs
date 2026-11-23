//! Accepting a suggestion starts a task as any start does: through the
//! person's model access and month spend limit (`admit_run`).

use crate::admin_app::tests::{server_with, Server};
use crate::config::Person;
use crate::events::Ev;
use crate::inbox::Answer;
use crate::kernel::NewRun;
use crate::spend::{Change, Scope};
use crate::state::Status;
use axum::Router;
use serde_json::json;
use std::time::Duration;

/// How often, and how many times, the test looks for a run to stop.
const POLL_EVERY: Duration = Duration::from_millis(20);
const POLLS: u32 = 500;

/// mo's run, stopped, then charged `usd`.
async fn charged_run(s: &Server, usd: f64) -> String {
    let (_, run) = s.k.create_run("person:mo", "mo", "take a note".into(), vec![], NewRun::default()).await.unwrap();
    for _ in 0..POLLS {
        if s.k.read(|st| st.runs[&run].status != Status::Running) {
            break;
        }
        tokio::time::sleep(POLL_EVERY).await;
    }
    let usage = magnus_adapter_proto::Usage { input_tokens: 1, output_tokens: 1, cache_read_tokens: 0, cache_write_tokens: 0, cost_usd: usd };
    s.k.emit("kernel", Ev::MemoryPass { run: run.clone(), model: "m".into(), blocks: vec![], usage }).await.unwrap();
    run
}

#[tokio::test(flavor = "multi_thread")]
async fn an_accepted_suggestion_stops_at_the_persons_month_limit() {
    let s = server_with("", |_| Router::new()).await;
    let run = charged_run(&s, 2.0).await;
    s.k.emit("person:sam", Ev::Spend(Change::LimitSet { scope: Scope::Person("mo".into()), usd: Some(1.0) })).await.unwrap();
    let r = s.k.read(|st| st.runs[&run].clone());
    let item = s.k.item(&r, "suggestion", "Magnus suggests: digest".into(), json!({ "suggestion": true, "title": "Digest", "text": "Summarise the week." })).await.unwrap();
    let tasks = s.k.read(|st| st.tasks.len());
    let mo = Person { id: "mo".into(), name: "Mo".into(), token: None, groups: vec![], roles: vec![] };
    let answer = s.k.respond(&mo, &item, &json!({ "decision": "accept" })).await.unwrap();
    assert!(matches!(answer, Answer::Invalid(_)), "a suggestion was accepted past mo's spent month limit");
    assert_eq!(s.k.read(|st| st.tasks.len()), tasks, "accepting started a task past the month limit");
    s.done().await;
}
