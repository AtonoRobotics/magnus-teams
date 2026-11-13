//! A watched read's poll is work done for its person through their own
//! sign-in, so every kill switch that covers it holds it
//! (decisions/2026-11-11-kill-switch-levels.md: a connector's switch
//! "refuses any call to it at the boundary", a person's covers "everything
//! done for that person"): the server's, each person's, each person's
//! agent's, and the app's. Nothing reaches the app while one is on.

use magnus_invariants::*;
use magnus_scratch::reconfigure;
use serde_json::json;
use std::time::Duration;

/// Past the first due sweep after the loop is turned on: a watch with a
/// one-minute interval that may poll has polled by then
/// (`watch_poll.rs::watches_the_person_cannot_use_are_not_polled`).
const PAST_FIRST_SWEEP: Duration = Duration::from_secs(20);

async fn no_poll_while(presses: &[&str]) {
    let mail = mcp::mail().start().await;
    let m = FakeModel::start(script(|_| async move { tool("task_fail", json!({ "reason": "no tasks in this test" })) })).await;
    // Boot with the loop off, so nothing polls before the switch is on.
    let mut app =
        Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("list_inbox", "read")]).watch_poll("mail", "list_inbox", "/messages", "subject", 1), &m).await;
    let alice = app.api("alice");
    for press in presses {
        let (st, v) = alice.post(press, json!({})).await;
        assert!(st.is_success(), "{press}: {v}");
    }
    reconfigure(&mut app, |cfg| {
        cfg["proactive"].as_table_mut().unwrap().insert("on".into(), toml::Value::Boolean(true));
    })
    .await;
    tokio::time::sleep(PAST_FIRST_SWEEP).await;
    assert!(mail.calls_to("list_inbox").is_empty(), "a watched read polled while {presses:?} held it");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_servers_switch_holds_every_poll() {
    no_poll_while(&["/v1/admin/kill-switch/press"]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_persons_switch_holds_their_polls() {
    no_poll_while(&["/v1/admin/kill-switch/person/alice/press", "/v1/admin/kill-switch/person/bob/press", "/v1/admin/kill-switch/person/carol/press"]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_persons_agents_switch_holds_their_polls() {
    no_poll_while(&["/v1/admin/kill-switch/agent/person:alice/press", "/v1/admin/kill-switch/agent/person:bob/press", "/v1/admin/kill-switch/agent/person:carol/press"]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_apps_switch_holds_its_polls() {
    no_poll_while(&["/v1/admin/kill-switch/connector/mail/press"]).await;
}
