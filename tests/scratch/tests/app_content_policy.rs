//! Writing a connection's content into shared org memory passes policy like
//! any other action (connectors spec, How the agent uses connectors). The
//! organisation forbids filing research from private content in the
//! company knowledge base; after reading Alice's calendar through her app,
//! filing what it said is refused by that rule, and nothing is filed. The
//! same filing from public web content goes through. (Who may read what an
//! app's content files is in knowledge.rs,
//! `nobody_retrieves_an_entry_they_may_not_read`.)

use magnus_invariants::*;
use magnus_scratch::FakeOAuthApp;
use serde_json::{json, Value};

const POLICY: &str = r#"
permit(principal, action == Action::"execute", resource)
  when { resource.risk == "read" || resource.risk == "reversible_write" || resource.risk == "sandbox" };
permit(principal, action == Action::"request_approval", resource);
@id("no_private_content_in_the_knowledge_base")
forbid(principal, action, resource == Tool::"knowledge_add")
  when { context.private };
"#;

const FROM_CALENDAR: &str = "File my calendar's offsite in the knowledge base.";
const FROM_WEB: &str = "File the public holiday list in the knowledge base.";

#[tokio::test(flavor = "multi_thread")]
async fn filing_an_apps_private_content_in_the_knowledge_base_is_decided_by_policy() {
    let cal = FakeOAuthApp::start(3600).await;
    let web = FakeWeb::start(&[("/holidays", "<p>The office closes on 25 December.</p>")]).await;
    let page = web.url("/holidays");
    let m = FakeModel::start(script(move |c| {
        let page = page.clone();
        async move {
            let from_calendar = c.body.to_string().contains(FROM_CALENDAR);
            match (c.turn(), from_calendar) {
                (0, _) => plan("file it"),
                (1, true) => tool("calendar__list_events", json!({})),
                (1, false) => tool("web_fetch", json!({ "url": page })),
                (2, true) => tool(
                    "knowledge_add",
                    json!({ "title": "Offsite", "text": "The team offsite is on the calendar.", "citations": [{ "source": "calendar", "quote": "Standup" }] }),
                ),
                (2, false) => tool(
                    "knowledge_add",
                    json!({ "title": "Holidays", "text": "The office closes on 25 December.", "citations": [{ "source": page, "quote": "The office closes on 25 December." }] }),
                ),
                _ => complete("done"),
            }
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    assert!(alice.put_policy("org", POLICY).await.is_success());
    let (st, v) = alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private+untrusted" })).await;
    assert!(st.is_success(), "{v}");
    assert_eq!(alice.put("/v1/connectors/calendar/tools/list_events", json!({ "risk": "read" })).await.0, 204);
    let (_, v) = alice.post("/v1/connectors/calendar/sign-in", json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
    let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    assert_eq!(alice.post("/v1/connectors/calendar/sign-in/complete", json!({ "state": state, "code": code })).await.0, 204);

    // From her calendar: refused by the rule, and nothing filed.
    let task = alice.submit(FROM_CALENDAR).await;
    alice.wait_status(&task, &["completed"]).await;
    let refused: String = m.requests().iter().filter(|c| c.body.to_string().contains(FROM_CALENDAR)).map(|c| c.text()).collect();
    assert!(refused.contains("Denied (policy)") && refused.contains("forbids knowledge_add"), "filing was not refused by the policy: {refused}");
    let (_, kb) = alice.get("/v1/knowledge?q=offsite").await;
    assert!(kb["entries"].as_array().is_some_and(|e| e.is_empty()), "the calendar's content was filed: {kb}");

    // From the public web: filed.
    let task = alice.submit(FROM_WEB).await;
    alice.wait_status(&task, &["completed"]).await;
    let (_, kb) = alice.get("/v1/knowledge?q=office+closes").await;
    let entries: Vec<Value> = kb["entries"].as_array().cloned().unwrap_or_default();
    assert_eq!(entries.len(), 1, "the web's content was not filed: {kb}");
}
