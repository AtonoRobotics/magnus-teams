//! An app's new tools put one review before the owner, however many runs
//! list them at once. Found by the autonomy acceptance (2026-11-09): the
//! server's first reviews for three people started together, each read the
//! mail tool as not pinned yet, and the owner got three identical "Review
//! the tool descriptions of mail" items.

use magnus_invariants::{mcp, *};
use serde_json::json;

/// Runs started together, each listing the app's tools as it starts.
const TOGETHER_COUNT: usize = 6;

#[tokio::test(flavor = "multi_thread")]
async fn runs_that_start_together_put_one_tool_review_before_the_owner() {
    let m = FakeModel::start(script(|_| async { Reply::Text("Done.".into()) })).await;
    let mail = mcp::mail().start().await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("list_inbox", "read")]), &m).await;
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
    let reviews: Vec<_> = alice.inbox().await.into_iter().filter(|i| i["kind"] == "tool_review").collect();
    assert_eq!(reviews.len(), 1, "the owner got the same tool review more than once: {reviews:?}");
    assert_eq!(reviews[0]["details"]["tools"][0]["tool"], "mail__list_inbox", "{reviews:?}");
}
