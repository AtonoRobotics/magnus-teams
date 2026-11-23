//! The core tools, carried out through the boundary on a kernel booted on a
//! fresh database.
use crate::admin_app::tests::{server_with, Server};
use crate::boundary::{Action, Issuer, Verdict};
use crate::events::Ev;
use crate::kernel::NewRun;
use crate::memory_index::MAX_MEMORY_WRITES_PER_RUN;
use crate::state::Status;
use axum::Router;
use serde_json::json;
use std::time::Duration;

/// How often, and how many times, a test looks for a run to stop.
const POLL_EVERY: Duration = Duration::from_millis(20);
const POLLS: u32 = 500;

/// mo's run, stopped by its driver (this server has no model) and set
/// running again with no driver, so the test's own actions are its next.
pub(crate) async fn idle_run(s: &Server) -> String {
    let (_, run) = s.k.create_run("person:mo", "mo", "take notes".into(), vec![], NewRun::default()).await.unwrap();
    for _ in 0..POLLS {
        if s.k.read(|st| st.runs[&run].status != Status::Running) {
            break;
        }
        tokio::time::sleep(POLL_EVERY).await;
    }
    s.k.emit("person:mo", Ev::Resumed { run: run.clone() }).await.unwrap();
    run
}

/// The model's next action on `run`, carried out through the boundary:
/// what it returned, and whether that was an error.
pub(crate) async fn act(s: &Server, run: &str, n: usize, tool: &str, input: serde_json::Value) -> (String, bool) {
    let a = Action { run: run.into(), call_id: format!("c{n}"), step_id: format!("s1.{n}"), tool: tool.into(), input, issuer: Issuer::Model };
    match s.k.execute(a).await.unwrap() {
        Verdict::Done { text, is_error, .. } => (text, is_error),
        Verdict::Suspended => panic!("{tool} was suspended"),
    }
}

async fn write(s: &Server, run: &str, n: usize) -> (String, bool) {
    act(s, run, n, "memory_write", json!({ "fact": format!("fact {n}") })).await
}

/// The cap counts what the run wrote: forgetting one of its memories does
/// not let it write another past the cap.
#[tokio::test(flavor = "multi_thread")]
async fn forgetting_a_memory_makes_no_room_under_the_runs_cap() {
    let s = server_with("", |_| Router::new()).await;
    let run = idle_run(&s).await;
    for n in 0..MAX_MEMORY_WRITES_PER_RUN {
        let (text, is_error) = write(&s, &run, n).await;
        assert!(!is_error, "write {n} was refused: {text}");
    }
    let first = s.k.read(|st| st.library.memory().iter().find(|m| m.run == run).map(|m| m.id.clone())).unwrap();
    s.k.emit("person:mo", Ev::MemoryForgotten { id: first }).await.unwrap();
    let (text, is_error) = write(&s, &run, MAX_MEMORY_WRITES_PER_RUN).await;
    assert!(is_error && text.contains("limit"), "the run wrote past its cap after forgetting one: {text}");
    let kept = s.k.read(|st| st.library.memory().iter().filter(|m| m.run == run).count());
    assert_eq!(kept, MAX_MEMORY_WRITES_PER_RUN - 1);
    s.done().await;
}
