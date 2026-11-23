//! One request is one task: the sub-agents a task starts, nested ones too,
//! are shown inside it and never listed among the person's tasks. The
//! HTTP list and the task's own view are checked end to end in the scratch
//! suite (`autonomy::a_scheduled_run_uses_sub_agents`); this test server's
//! database role cannot read the content those routes recall.
use crate::admin_app::tests::server_with;
use crate::events::Ev;
use crate::kernel::{new_id, NewRun};
use crate::sub_agents::SubAgent;
use axum::Router;

#[tokio::test(flavor = "multi_thread")]
async fn a_tasks_sub_agents_are_not_listed_beside_it() {
    let s = server_with("", |_| Router::new()).await;
    let k = &s.k;
    let (task, run) = k.create_run("person:mo", "mo", "research three suppliers".into(), vec![], NewRun::default()).await.unwrap();
    // Each child the way `start_sub_agent` makes it: the ledger first, then its run.
    let mut parent = run.clone();
    for goal in ["read supplier one's site", "read supplier two's site"] {
        let ids = (new_id("task"), new_id("run"));
        k.emit("kernel", Ev::SubAgent(SubAgent { child: ids.1.clone(), parent: parent.clone(), tools: vec![] })).await.unwrap();
        k.create_run(&format!("run:{parent}"), "mo", goal.into(), vec![], NewRun { ids: Some(ids.clone()), ..Default::default() }).await.unwrap();
        parent = ids.1;
    }
    let mo = k.person("mo").unwrap();
    let listed: Vec<String> = k.tasks_for(&mo, 30).await.iter().filter_map(|t| t["id"].as_str().map(String::from)).collect();
    assert_eq!(listed, [task], "one request was listed as several tasks");
    let held = k.read(|st| crate::chat_thread::sub_agents_of(st, &st.runs[&run], &Default::default()).len());
    assert_eq!(held, 2, "the task does not hold its sub-agents");
    s.done().await;
}
