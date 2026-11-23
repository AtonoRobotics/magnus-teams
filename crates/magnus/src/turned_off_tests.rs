use crate::events::Ev;
use chrono::Utc;
use serde_json::{json, Value};

fn ev(v: Value) -> Ev {
    serde_json::from_value(v).unwrap()
}

/// A schedule held on a run waiting on its person, when that person is
/// turned off, stays paused for them: once they are back on and answer the
/// run, the hold lifts and the turn-off's pause stays until they resume it
/// (decisions/2026-11-23-one-person-is-turned-off.md).
#[tokio::test(flavor = "multi_thread")]
async fn a_held_schedule_stays_paused_for_a_person_turned_off_and_back() {
    let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
    let db = crate::log::connect(&s.k.cfg.database.url).await.unwrap();
    db.batch_execute(&crate::log::content_walls_sql("postgres")).await.unwrap();
    let k = &s.k;
    let now = Utc::now();
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    k.emit("person:mo", ev(json!({ "kind": "schedule_set", "id": "sc1", "person": "mo", "text": "Summarise my inbox.", "every_minutes": 60 }))).await.unwrap();
    k.emit("schedule:sc1", ev(json!({ "kind": "task_created", "task": "t1", "run": "r1", "person": "mo", "text": "x", "config": config }))).await.unwrap();
    k.emit("kernel", ev(json!({ "kind": "schedule_fired", "id": "sc1", "task": "t1" }))).await.unwrap();
    k.emit("kernel", ev(json!({ "kind": "suspended", "run": "r1", "reason": "question" }))).await.unwrap();
    let schedule = || k.read(|s| s.arrangements.schedules()["sc1"].clone());
    k.sweep_schedules(now).await;
    assert!(schedule().held(), "the waiting run did not hold the schedule");

    k.turn_off("mo", "mo was turned off", "An administrator has turned your account off").await;
    k.emit("kernel", Ev::SignedIn { person: "mo".into(), session: "s1".into(), client: "browser".into() }).await.unwrap();
    assert!(!k.read(|s| s.identity.is_turned_off("mo")), "mo is not back on");
    k.emit("person:mo", ev(json!({ "kind": "resumed", "run": "r1" }))).await.unwrap();
    let later = now + chrono::Duration::hours(2);
    k.sweep_schedules(later).await;
    let sc = schedule();
    assert!(!sc.held(), "the answered run still holds the schedule");
    assert!(sc.paused.is_some() && sc.next_due().is_none(), "the schedule went on without its person resuming it: {:?}", sc.pauses);
    assert_eq!(k.read(|s| s.tasks.values().filter(|t| t.schedule.as_deref() == Some("sc1")).count()), 1, "the schedule fired");

    // Their resume lifts it.
    k.emit("person:mo", Ev::ScheduleResumed { id: "sc1".into(), source: Some("button".into()), turn: None }).await.unwrap();
    assert!(schedule().next_due().is_some(), "their resume left it paused");
    s.done().await;
}
