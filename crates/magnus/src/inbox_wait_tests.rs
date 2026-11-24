//! A card answers the wait it was made for
//! (decisions/2026-11-23-a-card-answers-its-own-wait.md): an answer lets
//! its run go on only while the run waits on that card, more budget comes
//! only from the notice the run is stopped on, and a card whose run has
//! ended is closed, so no answer to it grants, saves a rule or speaks to a
//! dead run.

use crate::admin_app::tests::{server_with, Server};
use crate::config::Person;
use crate::events::Ev;
use crate::inbox::Answer;
use crate::state::Status;
use axum::Router;
use serde_json::{json, Value};

fn mo() -> Person {
    Person { id: "mo".into(), name: "mo".into(), token: None, groups: vec![], roles: vec![] }
}

fn ev(v: Value) -> Ev {
    serde_json::from_value(v).unwrap()
}

/// mo's run `run` (task `t_<run>`), not driven.
async fn run(s: &Server, run: &str) {
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 9, "max_actions": 9, "max_usd": 4.0 }, "verifier": false, "instructions": "", "harness": "" });
    s.k.emit("person:mo", ev(json!({ "kind": "task_created", "task": format!("t_{run}"), "run": run, "person": "mo", "text": "Send the deck.", "config": config }))).await.unwrap();
}

/// A card of `kind` on `run`, and the run stopped on it.
async fn stopped_on(s: &Server, run: &str, item: &str, kind: &str, details: Value) {
    let card =
        json!({ "kind": "item", "id": item, "item_kind": kind, "task": format!("t_{run}"), "run": run, "person": "mo", "summary": "Send the deck to Bob?", "details": details });
    s.k.emit("kernel", ev(card)).await.unwrap();
    s.k.emit("kernel", ev(json!({ "kind": "suspended", "run": run, "reason": kind, "item": item }))).await.unwrap();
}

/// An ask Allow always is offered for: a new destination for mail.
fn ask(call: &str) -> Value {
    json!({ "call_id": call, "tool": "mail__send", "app": "mail", "risk": "irreversible", "rule": crate::asks::NEW_DESTINATION, "ask_destinations": ["bob@corp.example"] })
}

fn conflict(a: Answer) -> Option<&'static str> {
    match a {
        Answer::Conflict(why) => Some(why),
        _ => None,
    }
}

/// A scheduled run's ask went unanswered, so it went on (`unattended`) and
/// now waits until tomorrow. The person answering the old card later does
/// not wake it early: the run waits on the timer, not on the card.
#[tokio::test(flavor = "multi_thread")]
async fn an_answer_to_a_card_the_run_no_longer_waits_on_resumes_nothing() {
    let s = server_with("", |_| Router::new()).await;
    run(&s, "r1").await;
    stopped_on(&s, "r1", "i1", "approval", ask("c1")).await;
    // It went on without the answer, and then stopped on a timer.
    s.k.emit("kernel", ev(json!({ "kind": "resumed", "run": "r1" }))).await.unwrap();
    s.k.emit("model", ev(json!({ "kind": "wait", "run": "r1", "until": "2026-11-24T09:00:00Z" }))).await.unwrap();
    s.k.emit("kernel", ev(json!({ "kind": "suspended", "run": "r1", "reason": "timer" }))).await.unwrap();
    assert!(matches!(s.k.respond(&mo(), "i1", &json!({ "decision": "approve" })).await.unwrap(), Answer::Ok));
    let r = s.k.read(|st| st.runs["r1"].clone());
    assert_eq!((r.status, r.reason.as_deref()), (Status::Suspended, Some("timer")), "the answer woke a run waiting on a timer");
    assert!(r.wait_until.is_some(), "the answer cleared the run's wait");
    // A card the run does wait on still lets it go on.
    run(&s, "r2").await;
    stopped_on(&s, "r2", "i2", "question", json!({ "call_id": "c2" })).await;
    assert!(matches!(s.k.respond(&mo(), "i2", &json!({ "decision": "answer", "text": "Bob" })).await.unwrap(), Answer::Ok));
    assert_eq!(s.k.read(|st| st.runs["r2"].status), Status::Running, "the answer to the card the run waited on did not resume it");
    s.done().await;
}

/// The run stopped on its budget, and its task's Resume let it go on. Its
/// old notice's "more budget" doubles nothing now.
#[tokio::test(flavor = "multi_thread")]
async fn more_budget_from_the_notice_of_a_resumed_run_is_refused() {
    let s = server_with("", |_| Router::new()).await;
    run(&s, "r1").await;
    stopped_on(&s, "r1", "n1", "notice", json!({ "reason": "budget" })).await;
    s.k.emit("person:mo", ev(json!({ "kind": "resumed", "run": "r1" }))).await.unwrap();
    let before = s.k.read(|st| st.runs["r1"].config.budget.max_usd);
    let refused = conflict(s.k.respond(&mo(), "n1", &json!({ "decision": "extend" })).await.unwrap());
    assert!(refused.is_some(), "more budget was given to a run no longer stopped on the notice");
    assert_eq!(s.k.read(|st| st.runs["r1"].config.budget.max_usd), before, "the budget changed");
    assert!(s.k.read(|st| st.inbox.items()["n1"].resolution.is_none()), "a refused answer decided the notice");
    s.done().await;
}

/// Cancelled, a run's ask is closed: shown as such, refused when answered,
/// and Allow always from it saves no rule; its question tells it nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_runs_card_is_refused_and_allow_always_saves_no_rule() {
    let s = server_with("", |_| Router::new()).await;
    run(&s, "r1").await;
    stopped_on(&s, "r1", "i1", "approval", ask("c1")).await;
    s.k.emit(
        "kernel",
        ev(json!({ "kind": "item", "id": "q1", "item_kind": "question", "task": "t_r1", "run": "r1", "person": "mo", "summary": "Which deck?", "details": { "call_id": "c2" } })),
    )
    .await
    .unwrap();
    s.k.emit("person:mo", ev(json!({ "kind": "cancelled", "run": "r1" }))).await.unwrap();
    let shown = s.k.read(|st| crate::inbox::render(st, &mo(), &st.inbox.items()["i1"]));
    assert_eq!((shown["status"].as_str(), shown["can_respond"].as_bool()), (Some("resolved"), Some(false)), "{shown}");
    assert_eq!(shown["resolution"]["decision"], "superseded", "{shown}");
    let always = s.k.respond(&mo(), "i1", &json!({ "decision": "approve", "scope": "always" })).await.unwrap();
    assert_eq!(conflict(always), Some("the task this was for has ended"));
    assert!(s.k.read(|st| st.rules.of("mo").is_empty()), "Allow always on a cancelled run's card saved a rule");
    assert!(s.k.read(|st| st.inbox.approvals().is_empty()), "a cancelled run's card granted an approval");
    let answer = s.k.respond(&mo(), "q1", &json!({ "decision": "answer", "text": "The blue one" })).await.unwrap();
    assert_eq!(conflict(answer), Some("the task this was for has ended"));
    assert!(s.k.read(|st| !st.runs["r1"].results.contains_key("c2")), "an answer was told to a cancelled run");
    s.done().await;
}
