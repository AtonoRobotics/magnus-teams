//! Harness for the invariant tests.
//!
//! The tests reach the appliance only through its outside surfaces: the config
//! file, the `migrate` and `serve` commands, the local HTTP API, the Postgres
//! event log, and the external protocols it speaks. Every external system is a
//! fake we control: a model provider speaking the Anthropic Messages API, MCP
//! servers standing in for the person's apps, and a website. The fake model is
//! scripted, and often scripted to misbehave, because the invariants must hold
//! whatever the model decides.

pub mod appliance;
pub mod mcp;
pub mod model;
pub mod pg;
pub mod scan;
pub mod web;

pub use appliance::{Api, Appliance, Spec};
pub use mcp::FakeMcp;
pub use model::{script, steps, tool, tools, Conversation, FakeModel, Reply};
pub use web::FakeWeb;

use serde_json::{json, Value};
use std::time::Duration;

/// An organization policy that lets the agent read, make reversible writes and
/// use its sandbox on its own, and ask for everything else. Entities:
/// `Person::"<id>"` in `Group::"<id>"`; `Tool::"<app>__<tool>"` in
/// `App::"<app>"` with attributes `risk` and `audience`; actions
/// `Action::"execute"` (act without asking) and `Action::"request_approval"`.
pub const BASELINE: &str = r#"
permit(principal, action == Action::"execute", resource)
  when { resource.risk == "read" || resource.risk == "reversible_write" || resource.risk == "sandbox" };
permit(principal, action == Action::"request_approval", resource);
"#;

/// The baseline plus `extra` rules.
pub fn org_policy(extra: &str) -> String {
    format!("{BASELINE}\n{extra}\n")
}

/// A rule letting the agent run `tool` without asking.
pub fn permit_execute(tool: &str) -> String {
    format!("permit(principal, action == Action::\"execute\", resource == Tool::\"{tool}\");")
}

/// Default patience for anything the appliance does asynchronously.
pub const PATIENCE: Duration = Duration::from_secs(90);

/// A random marker that must never leak anywhere.
pub fn canary(tag: &str) -> String {
    format!("CANARY-{tag}-{}", uuid::Uuid::new_v4().simple())
}

/// A `task_contract` call with one criterion checked against the world:
/// `tool(input)` must return a result whose JSON value at `pointer` equals `equals`.
pub fn contract_checked(goal: &str, tool: &str, input: Value, pointer: &str, equals: Value) -> Reply {
    model::tool(
        "task_contract",
        json!({
            "goal": goal,
            "assumptions": [],
            "irreversible_actions": [],
            "criteria": [{
                "id": "c1",
                "description": format!("{tool} shows {pointer} = {equals}"),
                "check": { "tool": tool, "input": input, "expect": [{ "pointer": pointer, "equals": equals }] }
            }]
        }),
    )
}

/// A `task_contract` call whose only criterion has no executable check.
pub fn contract_unchecked(goal: &str, criterion: &str) -> Reply {
    model::tool(
        "task_contract",
        json!({
            "goal": goal,
            "assumptions": [],
            "irreversible_actions": [],
            "criteria": [{ "id": "c1", "description": criterion }]
        }),
    )
}

/// The example tasks' results are actions, so the deliverable is the record
/// of what was done: the summary, as one text component.
pub fn complete(summary: &str) -> Reply {
    model::tool("task_complete", json!({ "summary": summary, "deliverable": [{ "type": "text", "text": summary }] }))
}

pub fn fail(reason: &str) -> Reply {
    model::tool("task_fail", json!({ "reason": reason }))
}

/// Poll `f` until it returns `Some`, or panic after `timeout`.
pub async fn eventually<T, F, Fut>(what: &str, timeout: Duration, mut f: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let start = std::time::Instant::now();
    loop {
        if let Some(v) = f().await {
            return v;
        }
        if start.elapsed() > timeout {
            panic!("timed out after {timeout:?} waiting for: {what}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Assert that `f` stays `false` for the whole of `window`.
pub async fn never<F, Fut>(what: &str, window: Duration, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let start = std::time::Instant::now();
    while start.elapsed() < window {
        if f().await {
            panic!("must never happen, but did: {what}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
