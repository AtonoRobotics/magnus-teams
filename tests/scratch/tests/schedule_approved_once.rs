//! A schedule is approved once and then runs on its own (owner, 2026-11-02:
//! "When schedules are created they create a new task. Each task must be
//! approved which is not autonomous. The schedule should be approved once and
//! run until completion or it's stopped."). Its runs' policy asks are covered
//! by its confirmation, for actions its person could approve themselves; the
//! verifier judges the criteria no code checks, so no review waits on the
//! person. Someone who could not approve the action is still asked, and a
//! one-off task still asks as before.

use magnus_invariants::{mcp, *};
use magnus_scratch::reconfigure;
use serde_json::json;
use std::time::Duration;

const EMAIL: &str = "Email the team the plan.";

#[tokio::test(flavor = "multi_thread")]
async fn a_confirmed_schedule_runs_without_asking_again_within_its_persons_authority() {
    let mail = mcp::mail().start().await;
    let m = FakeModel::start(script(|c| async move {
        let t = c.text();
        // The verifier: reviews the criteria, then judges them true.
        if c.offered_tools().is_empty() && t.contains("The agent's criteria") {
            return Reply::Text("The criteria cover the request.".into());
        }
        if c.offered_tools().contains(&"verdict".to_string()) {
            return tool("verdict", json!({ "criteria": [{ "id": "c1", "holds": true, "evidence": "a message to the team is in sent" }] }));
        }
        if !t.contains(EMAIL) {
            return Reply::Text(String::new());
        }
        match c.turn() {
            0 => contract_unchecked("email the team the plan", "the team was sent the plan"),
            1 => tool("mail__send", json!({ "to": "team@corp.example", "subject": "The plan", "body": "Here is the plan." })),
            _ => complete("sent"),
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible"), ("list_sent", "read")]), &m).await;
    reconfigure(&mut app, |c| {
        let mut t = toml::Table::new();
        t.insert("min_every_minutes".into(), 1.into());
        c.insert("schedules".into(), toml::Value::Table(t));
    })
    .await;
    let (bob, carol) = (app.api("bob"), app.api("carol"));

    // Bob may approve irreversible actions: his schedule's run sends and
    // completes with nothing asked of him.
    let (st, v) = bob.post("/v1/schedules", json!({ "text": EMAIL, "every_minutes": 1 })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let run = |api: &Api| {
        let api = api.clone();
        async move {
            eventually("the schedule's run", Duration::from_secs(120), || async {
                api.tasks().await["tasks"].as_array().unwrap().iter().find(|t| t["text"] == EMAIL).and_then(|t| t["task_id"].as_str().map(String::from))
            })
            .await
        }
    };
    let task = run(&bob).await;
    let t = bob.wait_settled(&task).await;
    assert_eq!(t["status"], "completed", "bob's scheduled run did not complete on its own: {t}");
    assert!(bob.open_items(&task, "approval").await.is_empty() && bob.open_items(&task, "review").await.is_empty(), "bob's scheduled run asked him");
    assert!(mail.effects("send") >= 1, "the scheduled send did not happen");

    // Carol may not approve them: her schedule's run asks, as any task of hers.
    let (st, v) = carol.post("/v1/schedules", json!({ "text": EMAIL, "every_minutes": 1 })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let task = run(&carol).await;
    carol.wait_item(&task, "approval").await;

    // A one-off task of Bob's still asks before the irreversible send.
    let one = bob.submit(EMAIL).await;
    bob.wait_item(&one, "approval").await;
}
