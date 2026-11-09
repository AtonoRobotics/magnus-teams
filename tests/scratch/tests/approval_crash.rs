//! A decision on an inbox card, cut short by a crash (#194). The person's
//! decision, what it grants and the run's resumption are one write: a crash
//! at any point leaves either none of them or all of them, and after the
//! restart the run goes on, acting once.
//!
//! The crash is placed with the debug build's `crash-after` file in the
//! state directory: the appliance ends itself right after a write that put
//! an event of the named kind in the log.

use magnus_invariants::{mcp, *};
use serde_json::{json, Value};
use std::time::Duration;

/// Ends the appliance right after the next write that logs `kind`.
fn crash_after(app: &Appliance, kind: &str) {
    std::fs::write(app.state_dir.join("crash-after"), kind).unwrap();
}

/// Answer a card, through a crash: the appliance may end before it answers.
/// Then wait for the crash, and start it again.
async fn respond_through_crash(app: &mut Appliance, item: &str, body: Value) {
    let url = format!("{}/v1/inbox/{item}", app.base);
    let _ = reqwest::Client::new().post(url).bearer_auth(app.spec.token("alice")).json(&body).send().await;
    let path = app.state_dir.join("crash-after");
    eventually("the appliance crashes where the test placed it", Duration::from_secs(30), || {
        let crashed = !path.exists() && app.logs().contains("crash-after: ending the process");
        async move { crashed.then_some(()) }
    })
    .await;
    app.restart().await;
}

/// A task that sends one email, which waits on Alice's approval.
async fn waiting_on_approval() -> (FakeMcp, FakeModel, Appliance, String, Value) {
    let mail = mcp::mail().start().await;
    let model = FakeModel::start(steps(vec![
        contract_checked("send the report", "mail__list_sent", json!({}), "/count", json!(1)),
        tool("mail__send", json!({ "to": "bob@corp.example", "subject": "report", "body": "attached" })),
        complete("sent"),
    ]))
    .await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible"), ("list_sent", "read")]), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Email bob the report.").await;
    let item = alice.wait_item(&id, "approval").await;
    (mail, model, app, id, item)
}

/// After the restart: the run completes, the email went once, the card is
/// decided, and deciding it again is refused rather than applied twice.
async fn approved_once(app: &Appliance, mail: &FakeMcp, id: &str, item: &str) {
    let alice = app.api("alice");
    let t = eventually(&format!("task {id} completes after the restart"), Duration::from_secs(60), || {
        let alice = alice.clone();
        let id = id.to_string();
        async move {
            let t = alice.task(&id).await;
            (t["status"] == "completed" || t["status"] == "failed" || t["status"] == "cancelled").then_some(t)
        }
    })
    .await;
    assert_eq!(t["status"], "completed", "{t:#}\n{}", app.logs());
    assert_eq!(mail.effects("send"), 1, "the approved email was sent {} times", mail.effects("send"));
    let card = alice.inbox().await.into_iter().find(|i| i["id"] == item).expect("the card");
    assert_eq!(card["status"], "resolved", "{card:#}");
    assert_eq!(card["resolution"]["decision"], "approve", "{card:#}");
    assert!(alice.open_items(id, "approval").await.is_empty(), "the task asks again for what was approved");
    assert_eq!(alice.respond(item, json!({ "decision": "approve", "scope": "once" })).await, reqwest::StatusCode::CONFLICT);
    assert_eq!(mail.effects("send"), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crash_after_an_approval_is_decided_leaves_it_granted() {
    let (mail, _model, mut app, id, item) = waiting_on_approval().await;
    let item = item["id"].as_str().unwrap().to_string();
    crash_after(&app, "item_resolved");
    respond_through_crash(&mut app, &item, json!({ "decision": "approve", "scope": "once" })).await;
    approved_once(&app, &mail, &id, &item).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crash_after_an_approval_is_granted_still_resumes_its_run() {
    let (mail, _model, mut app, id, item) = waiting_on_approval().await;
    let item = item["id"].as_str().unwrap().to_string();
    crash_after(&app, "approval");
    respond_through_crash(&mut app, &item, json!({ "decision": "approve", "scope": "once" })).await;
    approved_once(&app, &mail, &id, &item).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crash_after_a_review_is_accepted_still_completes_its_run() {
    let model = FakeModel::start(steps(vec![contract_unchecked("write a poem", "the poem is moving"), complete("wrote it")])).await;
    let mut app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Write me a poem.").await;
    let review = alice.wait_item(&id, "review").await;
    let item = review["id"].as_str().unwrap().to_string();
    crash_after(&app, "item_resolved");
    respond_through_crash(&mut app, &item, json!({ "decision": "accept" })).await;
    let alice = app.api("alice");
    eventually(&format!("task {id} completes after the restart"), Duration::from_secs(60), || {
        let alice = alice.clone();
        let id = id.clone();
        async move { (alice.task(&id).await["status"] == "completed").then_some(()) }
    })
    .await;
    let card = alice.inbox().await.into_iter().find(|i| i["id"] == item.as_str()).expect("the card");
    assert_eq!(card["status"], "resolved", "{card:#}");
    assert_eq!(alice.respond(&item, json!({ "decision": "accept" })).await, reqwest::StatusCode::CONFLICT);
}
