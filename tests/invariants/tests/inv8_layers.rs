//! Invariant 8: policy is layered, organization down to user. The agent never
//! exceeds the rights of the person it acts for.
//!
//! Prevents: an agent with more access than the person who asked, and a lower
//! layer quietly widening what the organization allows.

use magnus_invariants::{mcp, *};
use serde_json::json;
use std::time::Duration;

fn close_42() -> Reply {
    tool("crm__update_record", json!({ "id": "42", "fields": { "status": "closed" } }))
}

fn crm_script() -> magnus_invariants::model::Script {
    steps(vec![contract_unchecked("close record 42", "closed"), close_42(), fail("done")])
}

async fn crm_app(model: &FakeModel) -> (FakeMcp, Appliance) {
    let crm = mcp::crm().start().await;
    let app = Appliance::start(Spec::default().app("crm", &crm, "internal", "private", &[("get_record", "read"), ("update_record", "reversible_write")]), model).await;
    (crm, app)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_user_layer_cannot_widen_the_organization_layer() {
    let mail = mcp::mail().start().await;
    let model = FakeModel::start(steps(vec![
        contract_unchecked("say hi", "said"),
        tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hi", "body": "hello" })),
        fail("done"),
    ]))
    .await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible")]), &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    assert!(alice.put_policy("org", &org_policy("")).await.is_success());
    assert!(bob.put_policy("user/bob", &permit_execute("mail__send")).await.is_success());

    let id = bob.submit("Email bob hello.").await;
    bob.wait_item(&id, "approval").await;
    assert_eq!(mail.effects("send"), 0, "a user-layer permit went past the organization's ceiling");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_organization_forbid_wins_over_any_lower_permit() {
    let model = FakeModel::start(crm_script()).await;
    let (crm, app) = crm_app(&model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let org = org_policy(r#"forbid(principal, action, resource == Tool::"crm__update_record");"#);
    assert!(alice.put_policy("org", &org).await.is_success());
    assert!(bob.put_policy("user/bob", &permit_execute("crm__update_record")).await.is_success());
    let id = bob.submit("Close CRM record 42.").await;
    bob.wait_settled(&id).await;
    assert_eq!(crm.effects("update_record"), 0);
    assert!(bob.open_items(&id, "approval").await.is_empty(), "a forbidden action was offered for approval");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_user_layer_narrows_for_that_user_only() {
    let model = FakeModel::start(crm_script()).await;
    let (crm, app) = crm_app(&model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    assert!(alice.put_policy("org", &org_policy("")).await.is_success());
    assert!(bob.put_policy("user/bob", r#"forbid(principal, action == Action::"execute", resource == Tool::"crm__update_record");"#).await.is_success());

    let b = bob.submit("Close CRM record 42.").await;
    bob.wait_settled(&b).await;
    assert_eq!(crm.effects("update_record"), 0, "bob's own narrowing was ignored");

    let a = alice.submit("Close CRM record 42.").await;
    alice.wait_settled(&a).await;
    assert_eq!(crm.effects("update_record"), 1, "bob's layer narrowed alice");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_group_layer_narrows_its_members() {
    let model = FakeModel::start(crm_script()).await;
    let (crm, app) = crm_app(&model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    assert!(alice.put_policy("org", &org_policy("")).await.is_success());
    assert!(alice.put_policy("group/finance", r#"forbid(principal, action, resource == Tool::"crm__update_record");"#).await.is_success());

    let b = bob.submit("Close CRM record 42.").await; // bob is in finance
    bob.wait_settled(&b).await;
    assert_eq!(crm.effects("update_record"), 0, "the finance group's layer did not apply to bob");

    let a = alice.submit("Close CRM record 42.").await; // alice is not
    alice.wait_settled(&a).await;
    assert_eq!(crm.effects("update_record"), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_never_exceeds_the_persons_own_access() {
    let finance = mcp::finance().start().await;
    let model =
        FakeModel::start(steps(vec![contract_unchecked("get the Q3 report", "report fetched"), tool("finance__get_report", json!({ "quarter": "Q3" })), fail("done")])).await;
    let app = Appliance::start(Spec::default().app("finance", &finance, "internal", "private", &[("get_report", "read")]).access("finance", &["finance"]), &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    // The organization lets the agent do anything at all.
    assert!(alice.put_policy("org", "permit(principal, action, resource);").await.is_success());

    let a = alice.submit("Get me the Q3 finance report.").await; // alice is not in finance
    alice.wait_settled(&a).await;
    assert!(finance.calls().is_empty(), "the agent used access alice does not have");
    assert!(alice.open_items(&a, "approval").await.is_empty(), "an approval was offered to get around alice's rights");

    let b = bob.submit("Get me the Q3 finance report.").await; // bob is
    bob.wait_settled(&b).await;
    assert_eq!(finance.calls_to("get_report").len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn only_holders_of_the_approver_role_can_approve() {
    let mail = mcp::mail().start().await;
    let model = FakeModel::start(steps(vec![
        contract_unchecked("say hi", "said"),
        tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hi", "body": "hello" })),
        fail("done"),
    ]))
    .await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible")]), &model).await;
    let (bob, carol) = (app.api("bob"), app.api("carol"));
    let id = carol.submit("Email bob hello.").await;
    let item = carol.wait_item(&id, "approval").await;
    let iid = item["id"].as_str().unwrap();

    let st = carol.respond(iid, json!({ "decision": "approve", "scope": "once" })).await;
    assert!(st == 403 || st == 404, "carol approved without the approver role: {st}");
    never("an approval without the role took effect", Duration::from_secs(3), || async { mail.effects("send") > 0 }).await;

    assert!(bob.respond(iid, json!({ "decision": "approve", "scope": "once" })).await.is_success());
    eventually("bob's approval took effect", PATIENCE, || async { (mail.effects("send") == 1).then_some(()) }).await;
}
