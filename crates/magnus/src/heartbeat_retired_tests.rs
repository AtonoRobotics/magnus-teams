//! The heartbeat is retired (decisions/2026-11-26-the-heartbeat-is-retired.md):
//! a heartbeat still set up ends when it comes due and starts no run, while
//! any other schedule fires as before.

use crate::events::Ev;
use crate::schedule_sweep::HEARTBEAT_RETIRED;
use chrono::Utc;
use serde_json::{json, Value};

fn ev(v: Value) -> Ev {
    serde_json::from_value(v).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_heartbeat_ends_when_due_and_starts_no_run() {
    let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
    let db = crate::log::connect(&s.k.cfg.database.url).await.unwrap();
    db.batch_execute(&crate::log::content_walls_sql("postgres")).await.unwrap();
    let k = &s.k;
    // One set up before schedules carried `system`, known by its words, and
    // one marked a system schedule.
    k.emit("person:mo", ev(json!({ "kind": "schedule_set", "id": "hb_old", "person": "mo", "title": "Heartbeat", "text": "Heartbeat: Check connected email, calendar, and active threads.", "every_minutes": 30 }))).await.unwrap();
    k.emit("person:mo", ev(json!({ "kind": "schedule_set", "id": "hb_sys", "person": "mo", "text": "Look over my day.", "every_minutes": 30, "system": true }))).await.unwrap();
    k.emit("person:mo", ev(json!({ "kind": "schedule_set", "id": "plain", "person": "mo", "text": "Summarise my inbox.", "every_minutes": 30 }))).await.unwrap();

    k.sweep_schedules(Utc::now() + chrono::Duration::hours(1)).await;

    for id in ["hb_old", "hb_sys"] {
        let sc = k.read(|s| s.arrangements.schedules()[id].clone());
        assert_eq!(sc.ended.as_deref(), Some(HEARTBEAT_RETIRED), "{id} did not end as retired");
        assert_eq!(k.read(|s| s.tasks.values().filter(|t| t.schedule.as_deref() == Some(id)).count()), 0, "{id} started a run");
    }
    let told = k.read(|s| s.inbox.items().values().filter(|n| n.person == "mo" && n.details["evidence"]["why"] == HEARTBEAT_RETIRED).count());
    assert_eq!(told, 2, "each heartbeat's person is told once why it ended");
    let plain = k.read(|s| s.arrangements.schedules()["plain"].clone());
    assert!(plain.ended.is_none(), "a schedule that is not a heartbeat ended");
    assert!(k.fire_schedule_now(k.read(|s| s.arrangements.schedules()["hb_old"].clone())).await.is_err(), "Run now started a retired heartbeat");
    s.done().await;
}
