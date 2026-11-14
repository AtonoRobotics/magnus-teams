//! Regressions from the review of master (2026-10-31), people's own tasks
//! and schedules. The owner is the admin, and each person has their own
//! account (owner, 2026-10-31):
//! - releasing the kill switch tells each person whose run it stopped, in
//!   their own inbox, and Continue there goes on; the admin is not shown
//!   their tasks;
//! - a schedule a run proposes never gets more than the appliance gives a
//!   run, even when the person extended that run;
//! - a schedule's own notice (it ended itself) has no Continue: answering it
//!   so is refused and does not move its run.

use magnus_invariants::{mcp, *};
use magnus_scratch::reconfigure;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread")]
async fn releasing_the_kill_switch_tells_each_person_whose_run_it_stopped() {
    let in_turn = Arc::new(AtomicBool::new(false));
    let go = Arc::new(tokio::sync::Notify::new());
    let (t, g) = (in_turn.clone(), go.clone());
    let m = FakeModel::start(script(move |c| {
        let (t, g) = (t.clone(), g.clone());
        async move {
            match c.turn() {
                0 => plan("write a note"),
                // Held open until the kill switch is engaged.
                1 if !t.swap(true, Ordering::SeqCst) => {
                    g.notified().await;
                    Reply::Text("Thinking.".into())
                }
                _ => complete("Written."),
            }
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));
    let id = carol.submit("Write a note.").await;
    eventually("carol's run is in a model call", PATIENCE, || {
        let t = in_turn.clone();
        async move { t.load(Ordering::SeqCst).then_some(()) }
    })
    .await;
    assert!(alice.post("/v1/kill-switch", json!({ "engaged": true })).await.0.is_success());
    go.notify_one();
    assert_eq!(carol.wait_status(&id, &["suspended"]).await["waiting_on"], "kill_switch");

    let (st, v) = alice.post("/v1/kill-switch", json!({ "engaged": false })).await;
    assert!(st.is_success(), "{v}");
    assert_eq!(v["told"], 1, "{v}");
    let n = eventually("carol is told her run was stopped", PATIENCE, || async {
        carol.inbox().await.into_iter().find(|i| i["kind"] == "notice" && i["task_id"] == json!(id) && i["details"]["reason"] == "kill_switch")
    })
    .await;
    assert!(!alice.inbox().await.iter().any(|i| i["id"] == n["id"]), "the admin was shown carol's task");
    assert!(matches!(alice.respond(n["id"].as_str().unwrap(), json!({ "decision": "resume" })).await.as_u16(), 403 | 404), "the admin resumed carol's task");
    // Nothing went on by itself; carol's Continue does.
    assert_eq!(carol.task(&id).await["status"], "suspended");
    assert!(carol.respond(n["id"].as_str().unwrap(), json!({ "decision": "resume" })).await.is_success());
    // It goes on to its end, and nothing is put to the admin.
    assert_eq!(carol.wait_status(&id, &["completed"]).await["status"], "completed");
    assert!(!alice.inbox().await.iter().any(|i| i["task_id"] == json!(id) && i["status"] == "open"), "the admin was shown carol's task");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_suggested_schedule_never_gets_more_than_a_run_is_given() {
    let mail = mcp::mail().start().await;
    let m = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => plan("watch the inbox"),
            1 | 2 => tool("mail__list_inbox", json!({})),
            3 => {
                let read = c.messages().into_iter().flat_map(|m| m["content"].as_array().cloned().unwrap_or_default()).find(|b| b["type"] == "tool_use" && b["name"] == "mail__list_inbox");
                let evidence = json!([{ "call": read.map(|b| b["id"].clone()), "shows": "New mail keeps arriving." }]);
                tool("suggest", json!({ "title": "Inbox summary", "goal": "Summarise the inbox.", "every_minutes": 60, "max_runs": 1, "permissions": ["Reads your inbox"], "evidence": evidence }))
            }
            _ => complete("Done."),
        }
    }))
    .await;
    let mut spec = Spec::default().app("mail", &mail, "public", "private", &[("list_inbox", "read")]);
    spec.max_model_calls = 3;
    let app = Appliance::start(spec, &m).await;
    let alice = app.api("alice");
    let t = alice.submit("Watch the inbox.").await;
    // Out of model calls; she gives this run twice as many.
    let n = alice.wait_item(&t, "notice").await;
    assert_eq!(n["details"]["reason"], "budget", "{n}");
    assert!(alice.respond(n["id"].as_str().unwrap(), json!({ "decision": "extend" })).await.is_success());
    let p = alice.wait_item(&t, "suggestion").await;
    assert!(alice.respond(p["id"].as_str().unwrap(), json!({ "decision": "accept" })).await.is_success());
    let s = alice.get("/v1/schedules").await.1["schedules"].as_array().unwrap().iter().find(|s| s["text"] == "Summarise the inbox.").cloned().expect("not scheduled");
    assert_eq!(s["budget"]["max_model_calls"], 3, "the run's extension became every scheduled run's: {s}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_schedules_own_notice_has_no_continue() {
    let m = FakeModel::start(script(|c| async move {
        if !c.body.to_string().contains("Check once.") {
            return complete("Nothing.");
        }
        match c.turn() {
            0 => plan("check once"),
            1 => tool("end_schedule", json!({ "reason": "done once" })),
            _ => complete("Checked."),
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    reconfigure(&mut app, |c| {
        let mut t = toml::Table::new();
        t.insert("min_every_minutes".into(), 1.into());
        c.insert("schedules".into(), toml::Value::Table(t));
    })
    .await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/schedules", json!({ "text": "Check once.", "every_minutes": 1 })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let n = eventually("the schedule ended itself", Duration::from_secs(100), || async {
        alice.inbox().await.into_iter().find(|i| i["kind"] == "notice" && i["details"]["reason"] == "schedule_ended")
    })
    .await;
    let task = n["task_id"].as_str().unwrap().to_string();
    // Its run ends, with nothing put to her.
    let before = alice.wait_settled(&task).await;
    assert_eq!(before["status"], "completed", "{before}");
    assert_eq!(alice.respond(n["id"].as_str().unwrap(), json!({ "decision": "resume" })).await.as_u16(), 400, "a schedule's notice was continued");
    let after = alice.task(&task).await;
    assert_eq!((&after["status"], &after["waiting_on"]), (&before["status"], &before["waiting_on"]), "the notice moved its run: {after}");
}
