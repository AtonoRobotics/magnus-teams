//! Tools searched across connectors, full schemas loaded on demand
//! (connectors spec, How the agent uses connectors). Past `LOAD_ALL` tools
//! across the person's connected apps, a run is offered none of them at
//! first, only `find_tools`; what it finds is offered, with its definition,
//! from its next step, and the call goes through. With fewer, every tool
//! is offered as before, and `find_tools` is not.

use magnus_invariants::mcp::{ok, FakeMcp};
use magnus_invariants::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// The tool names a model call was offered.
fn offered(body: &Value) -> Vec<String> {
    body["tools"].as_array().into_iter().flatten().filter_map(|t| t["name"].as_str().map(String::from)).collect()
}

async fn run_with(app_tools: usize) -> (Vec<Value>, FakeMcp) {
    let schema = json!({ "type": "object", "properties": {} });
    let mut b = FakeMcp::builder().tool("list_events", "List the events on the person's calendar.", schema.clone(), |_, _, _| ok(json!({ "events": [{ "title": "Standup" }] })));
    for i in 1..app_tools {
        b = b.tool(&format!("report_{i}"), &format!("Build sales report number {i}."), schema.clone(), |_, _, _| ok(json!({ "report": "done" })));
    }
    let ops = b.start().await;
    let seen: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(vec![]));
    let s = seen.clone();
    let m = FakeModel::start(script(move |c| {
        let s = s.clone();
        async move {
            s.lock().unwrap().push(c.body.clone());
            match c.turn() {
                0 => contract_unchecked("list the events", "the events are listed"),
                1 => tool("find_tools", json!({ "query": "calendar events" })),
                2 => tool("ops__list_events", json!({})),
                _ => complete("listed"),
            }
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/connectors", json!({ "name": "ops", "url": ops.url, "audience": "private", "label": "private+untrusted" })).await;
    assert!(st.is_success(), "{v}");
    let mut names = vec!["list_events".to_string()];
    names.extend((1..app_tools).map(|i| format!("report_{i}")));
    for t in &names {
        assert_eq!(alice.put(&format!("/v1/connectors/ops/tools/{t}"), json!({ "risk": "read" })).await.0, 204, "{t}");
    }
    let task = alice.submit("What is on my calendar?").await;
    alice.wait_item(&task, "review").await;
    let bodies = seen.lock().unwrap().clone();
    (bodies, ops)
}

#[tokio::test(flavor = "multi_thread")]
async fn past_the_limit_a_run_is_offered_the_tools_it_finds_and_they_go_through() {
    let (bodies, ops) = run_with(45).await;
    // The step that searches: no app tool is offered yet, and find_tools is.
    let before = offered(&bodies[1]);
    assert!(before.iter().any(|t| t == "find_tools"), "{before:?}");
    assert!(before.iter().all(|t| !t.starts_with("ops__")), "app tools were all offered past the limit: {before:?}");
    // What it found, as the model reads it.
    let result = bodies[2].to_string();
    assert!(result.contains("ops__list_events"), "find_tools did not find the calendar's tool: {result}");
    // From the next step, the found tool is offered with its definition; the rest are not.
    let after: Vec<Value> = bodies[2]["tools"].as_array().cloned().unwrap_or_default();
    let found = after.iter().find(|t| t["name"] == "ops__list_events").unwrap_or_else(|| panic!("the found tool is not offered: {:?}", offered(&bodies[2])));
    assert!(found["input_schema"].is_object() && found["description"].is_string(), "{found}");
    assert!(after.iter().all(|t| !t["name"].as_str().unwrap_or_default().starts_with("ops__report_")), "tools it did not find are offered: {:?}", offered(&bodies[2]));
    // Still offered at the step after, and the call went through.
    assert!(offered(&bodies[3]).iter().any(|t| t == "ops__list_events"), "{:?}", offered(&bodies[3]));
    assert_eq!(ops.calls_to("list_events").len(), 1, "the found tool's call did not go through");
}

#[tokio::test(flavor = "multi_thread")]
async fn within_the_limit_every_tool_is_offered_and_there_is_nothing_to_find() {
    let (bodies, ops) = run_with(5).await;
    let first = offered(&bodies[1]);
    assert!(first.iter().any(|t| t == "ops__list_events") && first.iter().any(|t| t == "ops__report_4"), "{first:?}");
    assert!(first.iter().all(|t| t != "find_tools"), "find_tools is offered with every tool already there: {first:?}");
    assert_eq!(ops.calls_to("list_events").len(), 1);
}
