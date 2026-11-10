//! Invariant 5: code only limits (deny, budget, retry, suspend). The model makes
//! the decisions.
//!
//! Prevents: workflows hidden in the harness that make the agent brittle, and
//! code that quietly ends, redirects or "fixes" the work.

// Test code: a failed unwrap is a failed test (AGENTS.md: none outside tests).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use magnus_invariants::{mcp, *};
use serde_json::{json, Value};
use std::time::Duration;

/// True if `v` appears as `input` of a requested tool call, or as the input of
/// a check written inside a requested contract.
fn requested(model: &FakeModel, tool: &str, args: &Value) -> bool {
    model.requested_calls().iter().any(|(name, input)| {
        (name == tool && input == args)
            || (name == "task_contract" && input["criteria"].as_array().into_iter().flatten().any(|c| c["check"]["tool"] == tool && &c["check"]["input"] == args))
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn every_app_call_traces_to_a_model_decision() {
    let crm = mcp::crm().start().await;
    let model = FakeModel::start(steps(vec![
        contract_checked("close record 42", "crm__get_record", json!({ "id": "42" }), "/status", json!("closed")),
        tool("crm__get_record", json!({ "id": "42" })),
        tool("crm__update_record", json!({ "id": "42", "fields": { "status": "closed" } })),
        complete("closed"),
    ]))
    .await;
    let app = Appliance::start(Spec::default().app("crm", &crm, "internal", "private", &[("get_record", "read"), ("update_record", "reversible_write")]), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Close CRM record 42.").await;
    let t = alice.wait_settled(&id).await;
    assert_eq!(t["status"], "completed", "{t}");
    assert!(!crm.calls().is_empty());
    for c in crm.calls() {
        assert!(requested(&model, &format!("crm__{}", c.tool), &c.args), "the kernel called {} {} on its own", c.tool, c.args);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_denial_goes_back_to_the_model_which_decides_what_next() {
    let crm = mcp::crm().start().await;
    let model = FakeModel::start(steps(vec![
        contract_unchecked("close record 42", "record closed"),
        tool("crm__update_record", json!({ "id": "42", "fields": { "status": "closed" } })),
        fail("the update was denied"),
    ]))
    .await;
    let app = Appliance::start(Spec::default().app("crm", &crm, "internal", "private", &[("get_record", "read"), ("update_record", "reversible_write")]), &model).await;
    let alice = app.api("alice");
    let policy = org_policy(r#"forbid(principal, action, resource == Tool::"crm__update_record");"#);
    assert!(alice.put_policy("org", &policy).await.is_success());
    let id = alice.submit("Close CRM record 42.").await;
    let t = alice.wait_settled(&id).await;

    assert!(crm.calls().is_empty(), "the kernel did something else in place of the denied call");
    let after = model.at_turn(2).expect("the model was asked again after the denial");
    let r = &after.last_results()[0];
    assert!(r.is_error, "the denial was not reported as an error");
    assert!(r.text.to_lowercase().contains("retry"), "the error does not say whether a retry can help: {}", r.text);
    assert_eq!(t["status"], "failed", "only the model's own declaration may end the run: {t}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_exhausted_budget_suspends_and_notifies() {
    let crm = mcp::crm().start().await;
    let model = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => contract_unchecked("look", "looked"),
            _ => tool("crm__get_record", json!({ "id": "42" })),
        }
    }))
    .await;
    let app = Appliance::start(Spec::default().app("crm", &crm, "internal", "private", &[("get_record", "read")]).max_model_calls(4), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Keep looking at record 42.").await;
    let t = alice.wait_settled(&id).await;
    assert_eq!(t["status"], "suspended", "{t}");
    alice.wait_item(&id, "notice").await;
    never("the model was called past its budget", Duration::from_secs(5), || async { model.requests().len() > 4 }).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_model_outage_suspends_the_run_and_resume_uses_the_pinned_model() {
    let model = FakeModel::start(steps(vec![Reply::Status(500)])).await;
    model.set_script(script(|_| async { Reply::Status(500) }));
    let app = Appliance::start(Spec::default().model_retries(2), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Say hello.").await;
    let t = alice.wait_settled(&id).await;
    assert_eq!(t["status"], "suspended", "a provider failure must suspend, not end, the run: {t}");
    alice.wait_item(&id, "notice").await;
    never("the kernel kept calling the failed provider", Duration::from_secs(3), || async { model.requests().len() > 3 }).await;

    model.set_script(steps(vec![contract_unchecked("say hello", "said"), fail("nothing to do")]));
    let (st, _) = alice.post(&format!("/v1/tasks/{id}/resume"), json!({})).await;
    assert!(st.is_success());
    alice.wait_status(&id, &["failed"]).await;
    assert!(model.requests().iter().all(|r| r.body["model"] == "claude-test"), "the run switched models");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reply_without_a_tool_call_does_not_end_the_run() {
    let model = FakeModel::start(steps(vec![Reply::Text("All done, I think.".into()), contract_unchecked("say hello", "said"), fail("nothing to do")])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Say hello.").await;
    let t = alice.wait_settled(&id).await;
    assert!(model.at_turn(1).is_some(), "the run ended on a text reply");
    assert_eq!(t["status"], "failed", "{t}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_principal_can_cancel_and_nothing_else_ends_a_run() {
    let model = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => contract_unchecked("wait", "waited"),
            _ => tool("wait_until", json!({ "at": "2099-01-01T00:00:00Z", "reason": "waiting for the year" })),
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Wait until 2099.").await;
    let t = alice.wait_settled(&id).await;
    assert_eq!(t["status"], "suspended", "{t}");
    let (st, _) = alice.post(&format!("/v1/tasks/{id}/cancel"), json!({})).await;
    assert!(st.is_success());
    alice.wait_status(&id, &["cancelled"]).await;
}
