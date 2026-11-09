//! Cedar governs the proactive loop's own outputs (#368): an org can
//! allow or deny `suggest`, `interrupt` and `act` per resource, and the
//! loop honors the denial — no observation, interruption or autonomous
//! act where denied. Suggestions and interruptions spend a person's
//! attention, so their resource is the person; acts touch apps, so
//! their resource is the tool, alongside the execute gate.

use magnus_invariants::model::Script;
use magnus_invariants::*;
use magnus_scratch::reconfigure;
use serde_json::{json, Value};
use std::time::Duration;

const REVIEW: &str = "reviewing one person's state";

fn done() -> Reply {
    tool("task_complete", json!({ "summary": "Reviewed quietly.", "deliverable": [{ "type": "text", "text": "Reviewed." }] }))
}

async fn drain_reviews(bob: &Api) {
    eventually("all reviews settled", Duration::from_secs(60), || {
        let me = bob.clone();
        async move {
            let (_, v) = me.get("/v1/tasks").await;
            let reviews: Vec<_> = v["tasks"].as_array().cloned().unwrap_or_default().into_iter().filter(|t| t["review"] == true).collect();
            (!reviews.is_empty() && reviews.iter().all(|t| t["status"] == "completed" || t["status"] == "failed")).then_some(())
        }
    })
    .await;
}

async fn enable_loop(app: &mut Appliance) {
    reconfigure(app, |cfg| {
        cfg["proactive"].as_table_mut().unwrap().insert("on".into(), toml::Value::Boolean(true));
    })
    .await;
}

/// A denied suggestion stops the observation, but the review itself
/// still completes: the denial is a policy outcome, not a failure.
#[tokio::test(flavor = "multi_thread")]
async fn a_suggest_deny_stops_observations_but_not_the_review() {
    let m = FakeModel::start(script(|c| async move {
        if !c.body["system"].to_string().contains(REVIEW) {
            return tool("task_fail", json!({ "reason": "not under test" }));
        }
        if c.turn() == 0 {
            return tool("observe", json!({ "what": "Something happened", "why": "It matters", "evidence": "somewhere" }));
        }
        done()
    }))
    .await;
    // The loop boots off so the policy lands before any review runs.
    let mut app = Appliance::start(Spec::default(), &m).await;
    assert!(app.api("alice").put_policy("org", &org_policy(r#"forbid(principal, action == Action::"suggest", resource);"#)).await.is_success());
    enable_loop(&mut app).await;
    let bob = app.api("bob");
    drain_reviews(&bob).await;
    let (_, v) = bob.get("/v1/tasks").await;
    assert!(v["tasks"].as_array().cloned().unwrap_or_default().iter().any(|t| t["review"] == true && t["status"] == "completed"), "the denied review did not complete: {v}");
    assert!(bob.inbox().await.iter().all(|i| !(i["kind"] == "observation" && i["person"] == "bob")), "a denied suggestion was queued");
}

/// A denied interruption degrades to an observation, like a spent
/// budget does: the news waits instead of interrupting.
#[tokio::test(flavor = "multi_thread")]
async fn an_interrupt_deny_queues_an_observation_instead() {
    let m = FakeModel::start(script(|c| async move {
        if !c.body["system"].to_string().contains(REVIEW) {
            return tool("task_fail", json!({ "reason": "not under test" }));
        }
        if c.turn() == 0 {
            return tool("interrupt", json!({ "what": "Something urgent happened", "why": "It cannot wait" }));
        }
        done()
    }))
    .await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    assert!(app.api("alice").put_policy("org", &org_policy(r#"forbid(principal, action == Action::"interrupt", resource);"#)).await.is_success());
    // Always in working hours with a fresh budget, so only the policy
    // stands between the interruption and the person.
    let (st, v) = app.api("bob").send(reqwest::Method::PUT, "/v1/preferences", Some(json!({ "working_hours": { "start": "00:00", "end": "23:59" } }))).await;
    assert!(st.is_success(), "{st} {v}");
    enable_loop(&mut app).await;
    let bob = app.api("bob");
    drain_reviews(&bob).await;
    let inbox = bob.inbox().await;
    assert!(inbox.iter().all(|i| !(i["kind"] == "interrupt" && i["person"] == "bob")), "a denied interruption was delivered: {inbox:?}");
    assert!(inbox.iter().any(|i| i["kind"] == "observation" && i["person"] == "bob"), "a denied interruption left nothing behind: {inbox:?}");
}

/// A denied suggestion stops the out-of-hours fallback too: with
/// interrupt allowed, an interruption outside working hours queues
/// nothing instead of bypassing through an observation (#402).
#[tokio::test(flavor = "multi_thread")]
async fn a_suggest_deny_stops_the_out_of_hours_fallback() {
    use chrono::Timelike;
    let m = FakeModel::start(script(|c| async move {
        if !c.body["system"].to_string().contains(REVIEW) {
            return tool("task_fail", json!({ "reason": "not under test" }));
        }
        if c.turn() == 0 {
            return tool("interrupt", json!({ "what": "Something urgent happened", "why": "It cannot wait" }));
        }
        done()
    }))
    .await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    assert!(app.api("alice").put_policy("org", &org_policy(r#"forbid(principal, action == Action::"suggest", resource);"#)).await.is_success());
    // A one-hour window two hours from now: never the test's own run.
    let h = (chrono::Utc::now().hour() as i64 + 2).rem_euclid(24);
    let (st, v) = app
        .api("bob")
        .send(reqwest::Method::PUT, "/v1/preferences", Some(json!({ "working_hours": { "start": format!("{h:02}:00"), "end": format!("{:02}:01", (h + 1).rem_euclid(24)) } })))
        .await;
    assert!(st.is_success(), "{st} {v}");
    enable_loop(&mut app).await;
    let bob = app.api("bob");
    drain_reviews(&bob).await;
    let inbox = bob.inbox().await;
    assert!(inbox.iter().all(|i| !(i["person"] == "bob" && (i["kind"] == "observation" || i["kind"] == "interrupt"))), "the fallback bypassed the suggest denial: {inbox:?}");
}

/// A denied suggestion stops the budget-spent fallback too: the first
/// three interruptions deliver, the fourth queues nothing (#402).
#[tokio::test(flavor = "multi_thread")]
async fn a_suggest_deny_stops_the_budget_spent_fallback() {
    let m = FakeModel::start(script(|c| async move {
        if !c.body["system"].to_string().contains(REVIEW) {
            return tool("task_fail", json!({ "reason": "not under test" }));
        }
        if c.turn() <= 3 {
            return tool("interrupt", json!({ "what": "Something urgent happened", "why": "It cannot wait" }));
        }
        done()
    }))
    .await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    assert!(app.api("alice").put_policy("org", &org_policy(r#"forbid(principal, action == Action::"suggest", resource);"#)).await.is_success());
    let (st, v) = app.api("bob").send(reqwest::Method::PUT, "/v1/preferences", Some(json!({ "working_hours": { "start": "00:00", "end": "23:59" } }))).await;
    assert!(st.is_success(), "{st} {v}");
    enable_loop(&mut app).await;
    let bob = app.api("bob");
    drain_reviews(&bob).await;
    let inbox = bob.inbox().await;
    let (interrupts, observations) =
        (inbox.iter().filter(|i| i["person"] == "bob" && i["kind"] == "interrupt").count(), inbox.iter().filter(|i| i["person"] == "bob" && i["kind"] == "observation").count());
    assert_eq!(interrupts, 3, "the budget did not spend as expected: {inbox:?}");
    assert_eq!(observations, 0, "the fallback bypassed the suggest denial: {inbox:?}");
}

/// A denied act stops that tool call in a loop run, per tool: the
/// forbidden call never executes, the allowed one beside it does.
#[tokio::test(flavor = "multi_thread")]
async fn an_act_deny_stops_loop_tool_calls_per_tool() {
    let mail = mcp::mail().start().await;
    let m = FakeModel::start(script(|c| async move {
        if !c.body["system"].to_string().contains(REVIEW) {
            return tool("task_fail", json!({ "reason": "not under test" }));
        }
        match c.turn() {
            0 => tool("mail__list_inbox", json!({})),
            1 => tool("mail__list_sent", json!({})),
            _ => done(),
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("list_inbox", "read"), ("list_sent", "read")]), &m).await;
    assert!(app.api("alice").put_policy("org", &org_policy(r#"forbid(principal, action == Action::"act", resource == Tool::"mail__list_inbox");"#)).await.is_success());
    enable_loop(&mut app).await;
    let bob = app.api("bob");
    drain_reviews(&bob).await;
    assert!(mail.calls_to("list_inbox").is_empty(), "a denied loop act executed");
    assert!(!mail.calls_to("list_sent").is_empty(), "an allowed loop act did not execute");
}
