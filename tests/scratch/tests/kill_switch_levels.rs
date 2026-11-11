//! The kill switch at its four levels in the admin's top bar (spec rev 141):
//! everything on the server, one person's agents, one agent, one connector.
//! Each stops running work at once (the run is stopped while its model call
//! is still out) and refuses new work in its scope; pressing needs no
//! step-up, releasing does; released, nothing resumes by itself, and the
//! stopped task continues from where it stopped. The banner (`/v1/stops`)
//! names who pulled it, to the people it affects.

use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// A model whose task marked HOLD stops in its model call after `first`
/// (a contract, then that reply) until `go`; every other task, and the held
/// one once let go, completes.
struct Held {
    in_turn: Arc<AtomicBool>,
    go: Arc<tokio::sync::Notify>,
    model: FakeModel,
}

async fn held_model(first: Option<Reply>) -> Held {
    let in_turn = Arc::new(AtomicBool::new(false));
    let go = Arc::new(tokio::sync::Notify::new());
    let (t, g) = (in_turn.clone(), go.clone());
    let model = start_checked(script(move |c| {
        let (t, g, first) = (t.clone(), g.clone(), first.clone());
        async move {
            let hold = c.text().contains("HOLD");
            let at = if first.is_some() { 2 } else { 1 };
            match c.turn() {
                0 => contract_unchecked("do the thing", "the thing is done"),
                1 if first.is_some() && hold => first.unwrap_or_else(|| complete("Done.")),
                n if n == at && hold => {
                    if !t.swap(true, Ordering::SeqCst) {
                        g.notified().await;
                    }
                    complete("Done.")
                }
                _ => complete("Done."),
            }
        }
    }))
    .await;
    Held { in_turn, go, model }
}

impl Held {
    async fn in_call(&self) {
        eventually("the task is in a model call", PATIENCE, || {
            let t = self.in_turn.clone();
            async move { t.load(Ordering::SeqCst).then_some(()) }
        })
        .await;
    }
}

/// The owner confirms their password, so a release goes through.
async fn stepped_up(app: &Appliance, admin: &Api) {
    assert!(set_password(app, "alice", PASSWORD));
    let (st, v) = admin.post("/v1/admin/step-up", json!({ "password": PASSWORD })).await;
    assert!(st.is_success(), "step-up: {v}");
}

fn stops(v: &Value) -> Vec<(String, String)> {
    v["stops"].as_array().into_iter().flatten().map(|s| (s["level"].as_str().unwrap_or_default().into(), s["target"].as_str().unwrap_or_default().into())).collect()
}

/// One level below the server, pulled on bob's work by the owner: bob's
/// running task stops while its model call is out, his new work is refused,
/// alice's is not, the banner names who pulled it, a release needs a
/// step-up, and released, bob continues his task to the end.
async fn a_level_holds_bob(level: &str, target: &str, reason: &str) {
    let h = held_model(None).await;
    let app = Appliance::start(Spec::default(), &h.model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let id = bob.submit("HOLD: write a note.").await;
    h.in_call().await;

    // Only an admin presses, and pressing asks for no step-up.
    let press = format!("/v1/admin/kill-switch/{level}/{target}/press");
    assert_eq!(bob.post(&press, json!({})).await.0.as_u16(), 403, "a member pressed a kill switch");
    let (st, v) = alice.post(&press, json!({})).await;
    assert!(st.is_success(), "press: {v}");
    let t = bob.wait_status(&id, &["suspended"]).await;
    assert_eq!(t["waiting_on"], reason, "{t}");

    // New work in its scope is refused; outside it, it starts.
    let (st, v) = bob.post("/v1/tasks", json!({ "text": "Write another note." })).await;
    assert_eq!(st.as_u16(), 409, "new work was accepted under the switch: {v}");
    assert!(v["error"].as_str().unwrap_or_default().contains("kill switch"), "{v}");
    let (st, v) = alice.post("/v1/tasks", json!({ "text": "Write a note." })).await;
    assert_eq!(st.as_u16(), 201, "the switch on bob's work held alice's: {v}");
    // Bob cannot continue it while it is on.
    assert_eq!(bob.post(&format!("/v1/tasks/{id}/resume"), json!({})).await.0.as_u16(), 409);

    // The banner, to bob, with who pulled it and when; not to carol.
    let (_, v) = bob.get("/v1/stops").await;
    assert_eq!(stops(&v), [(level.to_string(), target.to_string())], "{v}");
    assert_eq!(v["stops"][0]["by"], "admin:alice", "{v}");
    assert!(v["stops"][0]["at"].is_string(), "{v}");
    assert!(stops(&app.api("carol").get("/v1/stops").await.1).is_empty(), "carol was shown bob's switch");
    // The admin's list of what is on.
    let (_, v) = alice.get("/v1/admin/kill-switches").await;
    assert_eq!(v["on"][0]["target"], target, "{v}");

    // Releasing needs a step-up, and resumes nothing by itself.
    let release = format!("/v1/admin/kill-switch/{level}/{target}/release");
    let (st, v) = alice.post(&release, json!({})).await;
    assert_eq!((st.as_u16(), v["error"].as_str()), (403, Some("step_up")), "{v}");
    stepped_up(&app, &alice).await;
    let (st, v) = alice.post(&release, json!({})).await;
    assert!(st.is_success() && v["told"] == 1, "release: {v}");
    h.go.notify_one();
    never("a task resumed by itself after the release", Duration::from_secs(3), || async { bob.task(&id).await["status"] != "suspended" }).await;
    let notice = bob.inbox().await.into_iter().find(|i| i["kind"] == "notice" && i["details"]["reason"] == reason);
    assert!(notice.is_some(), "bob was not told which task the switch stopped");
    let (st, v) = bob.post("/v1/tasks", json!({ "text": "Write another note." })).await;
    assert_eq!(st.as_u16(), 201, "released, new work is still refused: {v}");
    assert!(bob.post(&format!("/v1/tasks/{id}/resume"), json!({})).await.0.is_success());
    bob.wait_status(&id, &["completed"]).await;
    assert!(stops(&bob.get("/v1/stops").await.1).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn one_persons_switch_stops_their_running_work_at_once_and_refuses_their_new_work() {
    a_level_holds_bob("person", "bob", "kill_switch:person:bob").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn one_agents_switch_stops_its_running_work_at_once_and_refuses_its_new_work() {
    a_level_holds_bob("agent", "person:bob", "kill_switch:agent:person:bob").await;
}

/// The server's level, from the admin's top bar: everything stops, nothing
/// new starts for anyone, and the banner reaches everyone.
#[tokio::test(flavor = "multi_thread")]
async fn the_servers_switch_from_the_top_bar_stops_everything_at_once() {
    let h = held_model(None).await;
    let app = Appliance::start(Spec::default(), &h.model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let id = bob.submit("HOLD: write a note.").await;
    h.in_call().await;
    let (st, v) = alice.post("/v1/admin/kill-switch/press", json!({})).await;
    assert!(st.is_success(), "press: {v}");
    assert_eq!(bob.wait_status(&id, &["suspended"]).await["waiting_on"], "kill_switch");
    for who in [&alice, &bob] {
        let (st, v) = who.post("/v1/tasks", json!({ "text": "Write a note." })).await;
        assert_eq!(st.as_u16(), 409, "new work was accepted under the server's switch: {v}");
    }
    assert_eq!(stops(&app.api("carol").get("/v1/stops").await.1), [("server".to_string(), String::new())]);
    let (st, v) = alice.post("/v1/admin/kill-switch/release", json!({})).await;
    assert_eq!((st.as_u16(), v["error"].as_str()), (403, Some("step_up")), "{v}");
    stepped_up(&app, &alice).await;
    assert!(alice.post("/v1/admin/kill-switch/release", json!({})).await.0.is_success());
    h.go.notify_one();
    never("a task resumed by itself after the release", Duration::from_secs(3), || async { bob.task(&id).await["status"] != "suspended" }).await;
    let (st, v) = bob.post("/v1/tasks", json!({ "text": "Write a note." })).await;
    assert_eq!(st.as_u16(), 201, "released, new work is still refused: {v}");
    assert!(bob.post(&format!("/v1/tasks/{id}/resume"), json!({})).await.0.is_success());
    bob.wait_status(&id, &["completed"]).await;
}

/// One connector, for every agent: a run that has used it stops at once,
/// its tools leave every run's list, a call to it is refused, and work that
/// does not need it goes on. Released, the stopped task continues and sends.
#[tokio::test(flavor = "multi_thread")]
async fn one_connectors_switch_takes_it_from_every_agent_and_stops_the_work_that_needs_it() {
    let mail = mcp::mail().start().await;
    let h = held_model(Some(tool("mail__list_inbox", json!({})))).await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("list_inbox", "read"), ("send", "irreversible")]), &h.model).await;
    let alice = app.api("alice");
    let id = alice.submit("HOLD: read my inbox.").await;
    h.in_call().await;
    assert!(!mail.calls_to("list_inbox").is_empty(), "the task did not use the connector before the switch");

    let (st, v) = alice.post("/v1/admin/kill-switch/connector/mail/press", json!({})).await;
    assert!(st.is_success(), "press: {v}");
    assert_eq!(v["stopped"].as_array().map(Vec::len), Some(1), "{v}");
    assert_eq!(alice.wait_status(&id, &["suspended"]).await["waiting_on"], "kill_switch:connector:mail");

    // Work that does not need it starts, and is not offered it.
    let other = alice.submit("Write a note.").await;
    alice.wait_status(&other, &["completed"]).await;
    let offered: Vec<String> = h
        .model
        .requests()
        .iter()
        .filter(|c| c.text().contains("Write a note.") && c.offered_tools().iter().any(|t| t == "task_complete"))
        .flat_map(|c| c.offered_tools())
        .collect();
    assert!(!offered.is_empty() && !offered.iter().any(|t| t.starts_with("mail__")), "a run was offered the switched-off connector: {offered:?}");
    assert_eq!(stops(&app.api("carol").get("/v1/stops").await.1), [("connector".to_string(), "mail".to_string())], "the banner did not reach everyone");

    stepped_up(&app, &alice).await;
    let (st, v) = alice.post("/v1/admin/kill-switch/connector/mail/release", json!({})).await;
    assert!(st.is_success() && v["told"] == 1, "release: {v}");
    h.go.notify_one();
    never("a task resumed by itself after the release", Duration::from_secs(3), || async { alice.task(&id).await["status"] != "suspended" }).await;
    assert!(alice.post(&format!("/v1/tasks/{id}/resume"), json!({})).await.0.is_success());
    alice.wait_status(&id, &["completed"]).await;
    let offered: Vec<String> = h
        .model
        .requests()
        .iter()
        .rev()
        .find(|c| c.text().contains("HOLD") && c.offered_tools().iter().any(|t| t == "task_complete"))
        .map(|c| c.offered_tools())
        .unwrap_or_default();
    assert!(offered.iter().any(|t| t.starts_with("mail__")), "released, the connector did not come back: {offered:?}");
}

/// A call to a connector switched off while the run was in its model call
/// is refused at the boundary, and the run stops there: nothing is sent.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_to_a_switched_off_connector_is_refused_and_stops_its_run() {
    let mail = mcp::mail().start().await;
    let in_turn = Arc::new(AtomicBool::new(false));
    let go = Arc::new(tokio::sync::Notify::new());
    let (t, g) = (in_turn.clone(), go.clone());
    let m = start_checked(script(move |c| {
        let (t, g) = (t.clone(), g.clone());
        async move {
            match c.turn() {
                0 => contract_unchecked("send a note", "note sent"),
                1 => {
                    if !t.swap(true, Ordering::SeqCst) {
                        g.notified().await;
                    }
                    tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hi", "body": "hello" }))
                }
                _ => complete("Sent."),
            }
        }
    }))
    .await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible")]), &m).await;
    let alice = app.api("alice");
    assert!(alice.put_policy("org", &org_policy(&permit_execute("mail__send"))).await.is_success());
    let id = alice.submit("Email bob a hello.").await;
    eventually("the task is in a model call", PATIENCE, || {
        let t = in_turn.clone();
        async move { t.load(Ordering::SeqCst).then_some(()) }
    })
    .await;
    assert!(alice.post("/v1/admin/kill-switch/connector/mail/press", json!({})).await.0.is_success());
    go.notify_one();
    assert_eq!(alice.wait_status(&id, &["suspended"]).await["waiting_on"], "kill_switch:connector:mail");
    assert!(mail.calls_to("send").is_empty(), "a call went to a switched-off connector");
}
