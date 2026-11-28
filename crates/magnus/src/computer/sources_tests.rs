use super::super::egress::policy::{classify, Learned};
use super::super::testing::{person, server};
use super::super::Status;
use crate::boundary::{Action, Issuer};
use crate::kernel::NewRun;
use serde_json::json;

/// A source Auto Review let through stays reachable from the person's
/// computer after its task, and a form there can be sent
/// (2026-11-28-gates-beyond-the-invariants).
#[tokio::test(flavor = "multi_thread")]
async fn a_granted_source_stays_for_the_next_task_and_takes_a_form() {
    let s = server().await;
    let id = s.made_by_hand(&person("mo")).await;
    let (_, run) = s.k.create_run("person:mo", "mo", "book the table".into(), vec![], NewRun::default()).await.unwrap();
    let a = Action {
        run,
        call_id: "c1".into(),
        step_id: "s1".into(),
        tool: crate::rule_effects::SOURCE_TOOL.into(),
        input: json!({ "host": "tables.example" }),
        issuer: Issuer::Model,
    };
    let (text, is_error, _) = s.k.add_destination_tool(&a).await.unwrap();
    assert!(!is_error, "{text}");
    let mut c = s.k.read(|st| st.computers.get(&id).cloned()).unwrap();
    assert!(c.destinations.iter().any(|h| h == "tables.example"), "the source was not kept: {:?}", c.destinations);
    assert!(c.task_destinations.is_empty(), "the source was granted to one task: {:?}", c.task_destinations);
    c.status = Status::Running;
    let ended = |_: &str| false;
    assert_eq!(classify(&[], &c, &Learned::new(), &ended, "tables.example", 443, "POST"), Ok("added at the boundary"), "the next task could not send a form there");
    s.done().await;
}
