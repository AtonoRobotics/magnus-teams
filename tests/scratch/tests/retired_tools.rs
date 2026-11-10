//! A retired tool leaves the agent at once (connectors spec, Remote MCP
//! connectors). Alice's calendar offers create_event to her run; once the
//! app stops listing it, her next run is not offered it, though its
//! definition stays screened; listed again unchanged, it is offered again
//! with nothing asked of anyone (spec rev 140).

use magnus_invariants::*;
use magnus_scratch::FakeOAuthApp;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

fn offered(body: &Value) -> Vec<String> {
    body["tools"].as_array().into_iter().flatten().filter_map(|t| t["name"].as_str().map(String::from)).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_the_app_no_longer_lists_leaves_the_agent_at_once() {
    let cal = FakeOAuthApp::start(3600).await;
    let seen: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(vec![]));
    let s = seen.clone();
    let m = FakeModel::start(script(move |c| {
        let s = s.clone();
        async move {
            if c.turn() == 0 {
                s.lock().unwrap().push(c.body.clone());
            }
            complete("done")
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private+untrusted" })).await;
    assert!(st.is_success(), "{v}");
    for t in ["list_events", "create_event"] {
        assert_eq!(alice.put(&format!("/v1/connectors/calendar/tools/{t}"), json!({ "risk": "read" })).await.0, 204);
    }
    let (_, v) = alice.post("/v1/connectors/calendar/sign-in", json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
    let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    assert_eq!(alice.post("/v1/connectors/calendar/sign-in/complete", json!({ "state": state, "code": code })).await.0, 204);

    let first_offer = |text: &'static str| {
        let (alice, seen) = (alice.clone(), seen.clone());
        async move {
            let n = seen.lock().unwrap().len();
            let task = alice.submit(text).await;
            alice.wait_settled(&task).await;
            offered(&seen.lock().unwrap()[n])
        }
    };
    let before = first_offer("What is on my calendar?").await;
    assert!(before.contains(&"calendar__create_event".to_string()), "{before:?}");

    // The app retires create_event; the kernel's listing of it lasts two seconds.
    cal.state.lock().unwrap().retired = vec!["create_event".into()];
    tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
    let after = first_offer("What else is on my calendar?").await;
    assert!(!after.contains(&"calendar__create_event".to_string()), "a retired tool is still offered: {after:?}");
    assert!(after.contains(&"calendar__list_events".to_string()), "{after:?}");

    // Listed again, unchanged: offered again, with no card for anyone.
    let reviews = |alice: Api| async move { alice.inbox().await.len() };
    let had = reviews(alice.clone()).await;
    cal.state.lock().unwrap().retired.clear();
    tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
    let again = first_offer("And tomorrow?").await;
    assert!(again.contains(&"calendar__create_event".to_string()), "{again:?}");
    assert_eq!(reviews(alice.clone()).await, had, "its return, unchanged, put something before the owner");
}
