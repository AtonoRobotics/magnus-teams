//! Someone else's task or schedule: an owner or admin stops it, and its
//! person is told; changing it stays theirs. sam owns the server, ana is an
//! admin, mo a member.
use super::Refused;
use crate::admin_app::tests::server_with;
use crate::events::Ev;
use axum::Router;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn an_admin_stops_someone_elses_work_and_changes_none_of_it() {
    let s = server_with("", |_| Router::new()).await;
    let k = &s.k;
    let (sam, ana, mo) = (k.person("sam").unwrap(), k.person("ana").unwrap(), k.person("mo").unwrap());
    let set = |id: &str| Ev::ScheduleSet {
        id: id.into(),
        person: "mo".into(),
        text: "check the books".into(),
        every_minutes: Some(60),
        daily_at: None,
        budget: None,
        card: None,
        label: None,
        title: None,
        until: None,
        max_runs: None,
        scope: None,
        thread: None,
    };
    k.emit("person:mo", set("sched_a")).await.unwrap();
    for who in [&sam, &ana] {
        assert!(matches!(k.edit_schedule(who, "sched_a", &json!({ "every_minutes": 5 }), None).await, Err(Refused::NotFound(_))), "{} changed mo's schedule", who.id);
        assert!(matches!(k.cancel_schedule(who, "sched_a", None).await, Err(Refused::NotFound(_))), "{} deleted mo's schedule", who.id);
    }
    assert!(k.read(|st| st.schedules.get("sched_a").is_some_and(|x| x.every_minutes == Some(60) && x.ended.is_none())), "mo's schedule changed");
    k.pause_schedule(&ana, "sched_a", None).await.unwrap();
    assert!(matches!(k.resume_schedule(&ana, "sched_a", None).await, Err(Refused::Denied(_))), "an admin resumed what was only paused");
    k.resume_schedule(&mo, "sched_a", None).await.unwrap();
    let (task, _) = k.create_run("person:mo", "mo", "draft the letter".into(), vec![], crate::kernel::NewRun::default()).await.unwrap();
    let stopped = k.cancel_task(&sam, &task, None).await.unwrap();
    assert_eq!(stopped, json!({ "cancelled": task }), "the owner was shown what mo's task did");
    let told: Vec<String> = k.read(|st| st.items.values().filter(|i| i.person == "mo" && i.kind == "notice").map(|i| i.summary.clone()).collect());
    assert!(told.iter().any(|t| t == "Sam stopped your task"), "mo was not told their task was stopped: {told:?}");
    assert!(told.iter().any(|t| t.starts_with("Paused by Ana")), "mo was not told their schedule was paused: {told:?}");
    // Their own stays theirs to change.
    k.edit_schedule(&mo, "sched_a", &json!({ "every_minutes": 120 }), None).await.unwrap();
    k.cancel_schedule(&mo, "sched_a", None).await.unwrap();
    s.done().await;
}
