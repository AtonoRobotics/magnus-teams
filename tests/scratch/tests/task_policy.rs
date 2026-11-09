//! The task layer (DECISIONS, Layering): below the organisation, a person's
//! groups and the person, each task may have a policy of its own, which
//! only narrows. A layer that forbids a tool for one task stops that task's
//! call, and no other task's.

use magnus_invariants::*;
use magnus_scratch::control;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread")]
async fn a_tasks_own_layer_forbids_what_it_names_for_that_task_alone() {
    let web = FakeWeb::start(&[("/page", "<p>hello</p>")]).await;
    let page = web.url("/page");
    // Each task waits to call its tool until the test has set the layer.
    let go = Arc::new(AtomicBool::new(false));
    let g = go.clone();
    let m = FakeModel::start(script(move |c| {
        let (page, g) = (page.clone(), g.clone());
        async move {
            match c.turn() {
                0 => {
                    while !g.load(Ordering::SeqCst) {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                    tool("web_fetch", json!({ "url": page }))
                }
                _ => fail("done"),
            }
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));

    let fenced = carol.submit("Read the page.").await;
    let other = carol.submit("Read the page too.").await;
    // Carol narrows her own task; someone else may not touch it.
    let layer = format!("task/{fenced}");
    let cedar = r#"forbid(principal, action, resource == Tool::"web_fetch");"#;
    assert_eq!(app.api("bob").put_policy(&layer, cedar).await.as_u16(), 403, "someone else set carol's task's policy");
    assert_eq!(carol.put_policy("task/task_nonexistent", cedar).await.as_u16(), 404);
    assert!(carol.put_policy(&layer, cedar).await.is_success());
    // She reads back what she set, and nobody else's task's layer is shown to
    // others; the console drafts and applies it like any layer (review of #157).
    assert_eq!(carol.get("/v1/policies").await.1["layers"][&layer], cedar, "carol cannot read her task's layer");
    assert!(app.api("bob").get("/v1/policies").await.1["layers"].get(&layer).is_none(), "bob reads carol's task's layer");
    let (st, v) = control(&app, "POST", "/v1/admin/policy/draft", json!({ "actor": "admin:alice", "layer": layer, "cedar": cedar })).await;
    assert_eq!(st, 200, "the console cannot draft a task's layer: {v}");
    let (st, v) = control(&app, "POST", "/v1/admin/policy/apply", json!({ "actor": "admin:alice", "layer": layer, "cedar": cedar })).await;
    assert_eq!(st, 200, "the console cannot apply a task's layer: {v}");
    go.store(true, Ordering::SeqCst);
    carol.wait_settled(&fenced).await;
    carol.wait_settled(&other).await;

    let results: String = m.requests().iter().filter(|c| c.body.to_string().contains("Read the page.")).flat_map(|c| c.last_results()).map(|r| r.text).collect();
    assert!(results.contains("Denied (policy)") && results.contains(&format!("{layer}:")), "the task's layer did not stop its call: {results}");
    assert_eq!(web.hits().iter().filter(|h| h.contains("/page")).count(), 1, "the layer reached another task, or stopped nothing: {:?}", web.hits());
    let _ = alice;
}
