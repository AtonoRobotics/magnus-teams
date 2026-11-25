//! Stop and Delete are different (owner, 2026-11-23): Stop ends a schedule
//! and keeps it, with its runs; Delete removes it.
use super::{Refused, STOPPED_BY_PERSON};
use crate::admin_app::tests::server_with;
use crate::events::Ev;
use axum::Router;

#[tokio::test(flavor = "multi_thread")]
async fn stopping_a_schedule_keeps_it_and_deleting_removes_it() {
    let s = server_with("", |_| Router::new()).await;
    let k = &s.k;
    let (mo, ana) = (k.person("mo").unwrap(), k.person("ana").unwrap());
    let set = Ev::ScheduleSet {
        id: "sched_b".into(),
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
        quarantine_schema: None,
        system: false,
    };
    k.emit("person:mo", set).await.unwrap();
    let (task, run) = k.create_run("schedule:sched_b", "mo", "check the books".into(), vec![], crate::kernel::NewRun::default()).await.unwrap();
    assert!(k.read(|st| st.tasks.get(&task).is_some_and(|t| t.schedule.as_deref() == Some("sched_b"))), "the run is not the schedule's");

    // Only its person, or someone who controls it, stops it.
    assert!(matches!(k.stop_schedule(&ana, "sched_b").await, Err(Refused::NotFound(_))), "an admin stopped mo's schedule as theirs to change");

    let report = k.stop_schedule(&mo, "sched_b").await.unwrap();
    assert_eq!(report["runs_stopped"], 1, "its run still going did not stop: {report}");
    let kept = k.read(|st| st.arrangements.schedules().get("sched_b").map(|x| x.ended.clone()));
    assert_eq!(kept, Some(Some(STOPPED_BY_PERSON.to_string())), "a stopped schedule was not kept, ended");
    assert!(k.read(|st| st.runs.get(&run).is_some_and(|r| r.status.terminal())), "its run went on");
    assert!(matches!(k.stop_schedule(&mo, "sched_b").await, Err(Refused::Conflict(_))), "an ended schedule was stopped again");

    // Delete removes it, stopped or not.
    k.cancel_schedule(&mo, "sched_b", None).await.unwrap();
    assert!(k.read(|st| st.arrangements.schedules().get("sched_b").is_none()), "a deleted schedule stayed");
    s.done().await;
}
