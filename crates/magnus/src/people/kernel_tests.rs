//! The directory and the authority through a booted server: sam owns it, ana
//! is a config admin, mo a member, and lee a person only the server keeps,
//! an admin in its own directory and nowhere else.
use super::Authority;
use crate::admin_app::tests::server_with;
use crate::controls::Refused;
use crate::events::Ev;
use axum::Router;
use serde_json::json;

fn schedule(id: &str) -> Ev {
    Ev::ScheduleSet {
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
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_server_keeps_its_own_people_and_shares_with_them() {
    let s = server_with("", |_| Router::new()).await;
    let k = &s.k;
    assert_eq!(k.people().ids(), ["ana", "lee", "mo", "sam"], "the directory is not everyone once");
    assert!(k.people().exists("lee") && !k.people().exists("ghost"));
    assert_eq!(k.people().owners().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["sam"]);
    assert_eq!(k.people().admins().await, ["ana", "lee", "sam"], "an admin in the server's own directory was not an admin");
    let (_, v) = s.get(&s.token("mo").await, "/v1/directory").await;
    let shown: Vec<&str> = v["people"].as_array().into_iter().flatten().filter_map(|p| p["id"].as_str()).collect();
    assert!(shown.contains(&"lee"), "a person the server keeps cannot be shared with: {v}");
    assert!(!shown.contains(&"mo"), "mo was offered themselves: {v}");
    s.done().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_admin_in_the_servers_own_directory_stops_others_work() {
    let s = server_with("", |_| Router::new()).await;
    let k = &s.k;
    let lee = k.person("lee").unwrap();
    assert!(!lee.roles.iter().any(|r| r == "admin"), "lee has an admin role here: the test needs one only in the directory");
    assert_eq!(k.authority(&lee).await, Authority::Admin);
    k.emit("person:mo", schedule("sched_a")).await.unwrap();
    let (task, _) = k.create_run("person:mo", "mo", "draft the letter".into(), vec![], crate::kernel::NewRun::default()).await.unwrap();
    assert!(k.tasks_for(&lee, 30).await.iter().any(|t| t["id"] == task.as_str()), "lee was not shown mo's task to stop");
    assert!(k.get_schedule(&lee, "sched_a").await.is_ok(), "lee was not shown mo's schedule");
    k.pause_schedule(&lee, "sched_a", None).await.unwrap();
    assert!(matches!(k.edit_schedule(&lee, "sched_a", &json!({ "every_minutes": 5 }), None).await, Err(Refused::NotFound(_))), "lee changed mo's schedule");
    k.resume_schedule(&mo_of(k), "sched_a", None).await.unwrap();
    // An admin's suspension: only an admin lifts it, and lee is one.
    k.emit("admin:ana", Ev::SchedulePaused { id: "sched_a".into(), reason: "Suspended".into(), task: None, admin: true, source: None, turn: None }).await.unwrap();
    assert!(matches!(k.resume_schedule(&mo_of(k), "sched_a", None).await, Err(Refused::Conflict(_))), "mo lifted an admin's suspension");
    k.resume_schedule(&lee, "sched_a", None).await.unwrap();
    assert_eq!(k.cancel_task(&lee, &task, None).await.unwrap(), json!({ "cancelled": task }), "lee could not stop mo's task");
    s.done().await;
}

fn mo_of(k: &crate::kernel::Kernel) -> crate::config::Person {
    k.person("mo").unwrap()
}
