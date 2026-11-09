use magnus_invariants::{mcp, *};
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn approve_after_restart() {
    let mail = mcp::mail().start().await;
    let model = FakeModel::start(steps(vec![
        contract_checked("send the report", "mail__list_sent", json!({}), "/count", json!(1)),
        tool("mail__send", json!({ "to": "bob@corp.example", "subject": "report", "body": "attached" })),
        complete("sent"),
    ]))
    .await;
    let mut app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible"), ("list_sent", "read")]), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Email bob the report.").await;
    let item = alice.wait_item(&id, "approval").await;
    app.restart().await;
    let st = alice.respond(item["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "once" })).await;
    println!("respond: {st}");
    let t = alice.wait_settled(&id).await;
    println!("TASK {t:#}");
    println!("TIMELINE {:#}", alice.get(&format!("/v1/tasks/{id}/timeline")).await.1);
    println!("INBOX {:#}", json!(alice.inbox().await));
    println!("LOGS {}", app.logs());
}
