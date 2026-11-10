//! Invariant 7: for every task, the agent writes its own done-criteria, and
//! results are checked against the world, not against the agent's report.
//!
//! Prevents: agents declaring completion early and marking work done untested.

// Test code: a failed unwrap is a failed test (AGENTS.md: none outside tests).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use magnus_invariants::{mcp, *};
use serde_json::json;

fn crm_spec(crm: &FakeMcp) -> Spec {
    Spec::default().app("crm", crm, "internal", "private", &[("get_record", "read"), ("update_record", "reversible_write")])
}

fn close_42() -> Reply {
    tool("crm__update_record", json!({ "id": "42", "fields": { "status": "closed" } }))
}

fn closed_42_contract() -> Reply {
    contract_checked("close record 42", "crm__get_record", json!({ "id": "42" }), "/status", json!("closed"))
}

#[tokio::test(flavor = "multi_thread")]
async fn no_completion_and_no_action_without_a_contract() {
    let crm = mcp::crm().start().await;
    let model = FakeModel::start(steps(vec![close_42(), complete("done"), fail("giving up")])).await;
    let app = Appliance::start(crm_spec(&crm), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Close CRM record 42.").await;
    let t = alice.wait_settled(&id).await;
    assert_eq!(crm.effects("update_record"), 0, "an action ran before the agent wrote its done-criteria");
    assert!(model.at_turn(1).unwrap().last_results()[0].is_error);
    assert!(model.at_turn(2).unwrap().last_results()[0].is_error, "completion was accepted without criteria");
    assert_eq!(t["status"], "failed", "{t}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_false_report_of_completion_is_caught_by_checking_the_world() {
    let crm = mcp::crm().start().await;
    let model = FakeModel::start(steps(vec![
        closed_42_contract(),
        complete("closed it"), // a lie: nothing was done
        close_42(),
        complete("closed it, really"),
    ]))
    .await;
    let app = Appliance::start(crm_spec(&crm), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Close CRM record 42.").await;
    let t = alice.wait_settled(&id).await;

    let after_lie = model.at_turn(2).expect("the run continued after the false report");
    assert!(after_lie.last_results()[0].is_error, "the false report was accepted");
    assert_eq!(t["status"], "completed", "{t}");
    let c1 = t["criteria"].as_array().unwrap().iter().find(|c| c["id"] == "c1").expect("criterion c1").clone();
    assert_eq!(c1["verified"], true, "{c1}");
    assert!(!c1["evidence"].is_null(), "the verification left no evidence: {c1}");
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_reflects_the_world_at_the_time_of_checking() {
    let crm = mcp::crm().start().await;
    let c = crm.clone();
    let model = FakeModel::start(script(move |conv| {
        let crm = c.clone();
        async move {
            match conv.turn() {
                0 => closed_42_contract(),
                1 => close_42(),
                2 => {
                    // Someone reopens the record before the agent reports.
                    crm.set_world("records", json!({ "42": { "id": "42", "status": "open" } }));
                    complete("closed")
                }
                _ => fail("it was reopened"),
            }
        }
    }))
    .await;
    let app = Appliance::start(crm_spec(&crm), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Close CRM record 42.").await;
    let t = alice.wait_settled(&id).await;
    assert_ne!(t["status"], "completed", "the report was trusted over the world: {t}");
}

/// Checking is internal (spec rev 140): a criterion no check can confirm is
/// never put to the person. The task completes with the result, and the
/// criterion is recorded as unconfirmed for the developer view.
#[tokio::test(flavor = "multi_thread")]
async fn a_criterion_without_a_check_is_never_put_to_the_person() {
    let model = FakeModel::start(steps(vec![contract_unchecked("write a poem", "the poem is moving"), complete("wrote it"), fail("unexpected turn")])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");

    let id = alice.submit("Write me a poem.").await;
    let t = alice.wait_settled(&id).await;
    assert_eq!(t["status"], "completed", "{t}");
    assert!(alice.inbox().await.iter().all(|i| i["kind"] != "review"), "the person was asked to review criteria");
    assert!(app.db.dump().await.contains("Not confirmed by any check: c1"), "the unconfirmed criterion is not recorded");
}

#[tokio::test(flavor = "multi_thread")]
async fn checks_may_only_read() {
    let crm = mcp::crm().start().await;
    let model = FakeModel::start(steps(vec![
        // A "check" that would itself change the world.
        contract_checked("close record 42", "crm__update_record", json!({ "id": "42", "fields": { "status": "closed" } }), "/status", json!("closed")),
        complete("done"),
        fail("giving up"),
    ]))
    .await;
    let app = Appliance::start(crm_spec(&crm), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Close CRM record 42.").await;
    let t = alice.wait_settled(&id).await;
    assert!(model.at_turn(1).unwrap().last_results()[0].is_error, "a contract with a writing check was accepted");
    assert_eq!(crm.effects("update_record"), 0, "a check changed the world");
    assert_ne!(t["status"], "completed");
}
