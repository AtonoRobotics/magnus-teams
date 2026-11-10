//! An app's tools are screened once, however many runs list them at once,
//! and nothing about them reaches a person (spec rev 140, "Connecting an
//! app grants its use"; decisions/2026-11-10-connect-consent.md). Found by
//! the autonomy acceptance (2026-11-09) under the old per-tool review: the
//! server's first reviews for three people started together, each read the
//! mail tool as not seen yet, and the owner got three identical "Review the
//! tool descriptions of mail" items. Now each run screens the definition
//! before the agent sees it, one at a time, and the developer view holds
//! the one screen.

use magnus_invariants::appliance::Person;
use magnus_invariants::{mcp, *};
use serde_json::{json, Value};

/// Runs started together, each listing the app's tools as it starts.
const TOGETHER_COUNT: usize = 6;

#[tokio::test(flavor = "multi_thread")]
async fn runs_that_start_together_screen_a_tool_once_and_ask_no_one() {
    let m = FakeModel::start(script(|_| async { Reply::Text("Done.".into()) })).await;
    let mail = mcp::mail().start().await;
    let mut spec = Spec::default().app("mail", &mail, "public", "private", &[("list_inbox", "read")]);
    spec.people.push(Person { id: "dana".into(), name: "Dana".into(), token: "tok-dana-screens".into(), groups: vec!["staff".into()], roles: vec!["developer".into()] });
    let app = Appliance::start(spec, &m).await;
    let alice = app.api("alice");
    let turns = futures::future::join_all((0..TOGETHER_COUNT).map(|i| {
        let alice = alice.clone();
        async move {
            let (st, v) = alice.post("/v1/chat", json!({ "text": format!("Hello {i}.") })).await;
            assert_eq!(st.as_u16(), 201, "{v}");
            v["task_id"].as_str().unwrap().to_string()
        }
    }))
    .await;
    for t in &turns {
        alice.wait_settled(t).await;
    }
    let (st, v) = app.api("dana").get("/v1/dev/tool-screens").await;
    assert!(st.is_success(), "{v}");
    let screens: Vec<&Value> = v["screens"].as_array().into_iter().flatten().filter(|s| s["tool"] == "mail__list_inbox").collect();
    assert_eq!(screens.len(), 1, "the same definition was screened more than once: {v}");
    assert_eq!(screens[0]["verdict"], "phrases", "{v}");
    // Nothing about the app's tools reached anyone: no review, no card.
    let inbox = alice.inbox().await;
    assert!(inbox.iter().all(|i| !["tool_review", "tool_change", "app_tools"].contains(&i["kind"].as_str().unwrap_or_default())), "{inbox:?}");
    // The agent was offered the tool with its own description, screened.
    let offered = m
        .requests()
        .iter()
        .any(|c| c.body["tools"].as_array().into_iter().flatten().any(|t| t["name"] == "mail__list_inbox" && !t["description"].as_str().unwrap_or_default().is_empty()));
    assert!(offered, "the screened tool was not offered");
}
