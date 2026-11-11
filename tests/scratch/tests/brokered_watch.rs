//! A brokered app's watched read polls on its interval and wakes on new
//! items (decisions/2026-11-10-catalog-watched-reads.md): the test server
//! (`brokered/test-server`) in its microVM lists what a local HTTPS host,
//! standing in for the provider, lists at `/items`. The poll calls the
//! read with the watch's arguments, the first answer seeds silently, and
//! an item the provider lists later wakes one review naming the count,
//! with nobody reading the app.

use magnus_invariants::*;
use magnus_scratch::brokered::*;
use magnus_scratch::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const REVIEW: &str = "reviewing one person's state";
const ARGS: &str = "exactly these arguments: ";

fn done(summary: &str) -> Reply {
    tool("task_complete", json!({ "summary": summary, "deliverable": [{ "type": "text", "text": summary }] }))
}

/// The arguments a poll's text names, wherever in the request it is.
fn poll_args(v: &Value) -> Option<Value> {
    match v {
        Value::String(text) => {
            let rest = &text[text.find(ARGS)? + ARGS.len()..];
            serde_json::Deserializer::from_str(rest).into_iter::<Value>().next()?.ok()
        }
        Value::Array(a) => a.iter().find_map(poll_args),
        Value::Object(o) => o.values().find_map(poll_args),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_brokered_watch_polls_with_its_arguments_and_wakes_on_new_items() {
    let api = ProviderApi::start().await;
    *api.items.lock().unwrap() = json!([{ "id": "m1", "changed": "t1" }]).to_string();
    let reviews = Arc::new(Mutex::new(Vec::<String>::new()));
    let polls = Arc::new(Mutex::new(Vec::<Value>::new()));
    let (r, p) = (reviews.clone(), polls.clone());
    let m = FakeModel::start(script(move |c| {
        let (r, p) = (r.clone(), p.clone());
        async move {
            if c.body["system"].to_string().contains(REVIEW) {
                r.lock().unwrap().push(c.text());
                return done("Reviewed quietly.");
            }
            let text = c.text();
            match (text.contains("Poll testapp:"), c.turn()) {
                (true, 0) => {
                    let args = poll_args(&c.body["messages"]).unwrap_or(Value::Null);
                    p.lock().unwrap().push(args.clone());
                    tool("testapp__list", args)
                }
                _ => tool("task_fail", json!({ "reason": "polled" })),
            }
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default().proactive(json!({ "on": true })), &m).await;
    let provider = FakeOAuthApp::provider_api(3600).await;
    let image = image("magnus-brokered-test");
    let input = json!({ "host": api.host, "path": "/items" });
    reconfigure(&mut app, |cfg| {
        configure(cfg, &image, 300, "testapp", std::slice::from_ref(&api.host), &provider.base, &[("list", "read")]);
        trust(cfg, &api.ca);
        let watch: toml::Value = toml::Value::try_from(json!([{ "tool": "list", "items": "/items", "id": "id+changed", "poll_minutes": 1, "input": input }])).unwrap();
        let apps = cfg.get_mut("connectors").and_then(|c| c.as_array_mut()).unwrap();
        apps.last_mut().unwrap().as_table_mut().unwrap().insert("watch".into(), watch);
    })
    .await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/connectors/testapp/sign-in", json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
    assert!(st.is_success(), "{v}");
    let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    assert_eq!(alice.post("/v1/connectors/testapp/sign-in/complete", json!({ "state": state, "code": code })).await.0, 204);

    // The first poll reads the provider with the watch's arguments, and
    // seeds what was there without waking anyone.
    let listed = || api.seen.lock().unwrap().iter().filter(|(path, _)| path == "/items").count();
    eventually("the first poll read the provider", Duration::from_secs(150), || async { (listed() > 0).then_some(()) }).await;
    assert_eq!(polls.lock().unwrap().first(), Some(&input), "the poll called the read with the watch's arguments");
    let woke = |n: &str| reviews.lock().unwrap().iter().any(|t| t.contains(&format!("{n} new in testapp")));
    assert!(!woke("1"), "the seeding poll woke a review");

    // An item changes and another arrives; nobody reads the app. The next
    // poll wakes one review naming both.
    *api.items.lock().unwrap() = json!([{ "id": "m1", "changed": "t2" }, { "id": "m2", "changed": "t1" }]).to_string();
    eventually("the poll woke a review", Duration::from_secs(150), || async { woke("2").then_some(()) }).await;
    let (_, v) = alice.get("/v1/tasks").await;
    assert!(v["tasks"].as_array().cloned().unwrap_or_default().iter().all(|t| t["poll"] != true), "a poll task reached the person's list");
}
