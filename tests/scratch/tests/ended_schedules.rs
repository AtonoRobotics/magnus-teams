//! An ended schedule stays listed, marked with why, its latest run
//! beside it; only a cancelled one is gone. It refuses pause and resume,
//! and an edit that moves its end puts it back (#264).

use magnus_invariants::*;
use magnus_scratch::reconfigure;
use serde_json::{json, Value};
use std::time::Duration;

async fn schedules(alice: &Api, id: &str) -> Value {
    let (_, s) = alice.get("/v1/schedules").await;
    s["schedules"].as_array().unwrap().iter().find(|x| x["id"] == id).cloned().unwrap_or(Value::Null)
}

/// A schedule past its end stays listed, marked with why: not gone, and
/// not calling itself cancelled.
#[tokio::test(flavor = "multi_thread")]
async fn an_ended_schedule_stays_listed_with_why() {
    let m = FakeModel::start(script(|_| async { Reply::Text("unused".into()) })).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let until = (chrono::Utc::now() + chrono::Duration::seconds(3)).to_rfc3339();
    let (st, v) = alice.post("/v1/schedules", json!({ "text": "Say hi.", "every_minutes": 60, "until": until })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    let sc = eventually("the schedule to end", PATIENCE, || async {
        let sc = schedules(&alice, &id).await;
        (!sc.is_null() && !sc["ended"].is_null()).then_some(sc)
    })
    .await;
    assert!(sc["ended"].as_str().unwrap().contains("reached its end"), "no reason: {sc}");
    assert!(sc["next"].is_null(), "an ended schedule has a next run: {sc}");
}

/// An ended schedule refuses pause and resume, and an edit that moves its
/// end puts it back.
#[tokio::test(flavor = "multi_thread")]
async fn an_ended_schedule_refuses_pause_and_resume_but_takes_an_edit() {
    let m = FakeModel::start(script(|_| async { Reply::Text("unused".into()) })).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let until = (chrono::Utc::now() + chrono::Duration::seconds(3)).to_rfc3339();
    let (st, v) = alice.post("/v1/schedules", json!({ "text": "Say hi.", "every_minutes": 60, "until": until })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    eventually("the schedule to end", PATIENCE, || async {
        let sc = schedules(&alice, &id).await;
        (!sc.is_null() && !sc["ended"].is_null()).then_some(())
    })
    .await;
    assert_eq!(alice.post(&format!("/v1/schedules/{id}/pause"), json!({})).await.0.as_u16(), 409);
    assert_eq!(alice.post(&format!("/v1/schedules/{id}/resume"), json!({})).await.0.as_u16(), 409);
    // A new end puts it back: listed, unmarked, due again.
    let later = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    let (st, v) = alice.send(reqwest::Method::PATCH, &format!("/v1/schedules/{id}"), Some(json!({ "until": later }))).await;
    assert_eq!(st.as_u16(), 200, "{v}");
    let sc = schedules(&alice, &id).await;
    assert!(sc["ended"].is_null(), "the edit kept the end mark: {sc}");
    assert!(!sc["next"].is_null(), "the revived schedule is not due: {sc}");
}

/// A schedule out of runs ends with its latest run beside it: the run it
/// started, linked from the listing.
#[tokio::test(flavor = "multi_thread")]
async fn a_schedule_out_of_runs_ends_with_its_latest_run() {
    let m = FakeModel::start(script(|c| async move {
        if c.body["system"].to_string().contains("talking with the person") {
            return Reply::Text("On it.".into());
        }
        match c.turn() {
            0 => contract_unchecked("say hi", "hi was said"),
            _ => complete("hi"),
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
    let (st, v) = alice.post("/v1/schedules", json!({ "text": "Say hi.", "every_minutes": 1, "max_runs": 1 })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    let task = eventually("the schedule to fire", Duration::from_secs(150), || async {
        alice.tasks().await["tasks"].as_array().unwrap().iter().find(|t| t["text"] == "Say hi.").and_then(|t| t["task_id"].as_str().map(String::from))
    })
    .await;
    let sc = eventually("the schedule to run out", PATIENCE, || async {
        let sc = schedules(&alice, &id).await;
        (!sc.is_null() && !sc["ended"].is_null()).then_some(sc)
    })
    .await;
    assert!(sc["ended"].as_str().unwrap().contains("1 time"), "no reason: {sc}");
    assert_eq!(sc["last"]["task_id"].as_str(), Some(task.as_str()), "the listing lost the latest run: {sc}");
}
