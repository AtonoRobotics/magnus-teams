//! Model context reflects the capabilities the appliance actually offers.

use magnus_invariants::*;
use serde_json::json;

/// The system prompt of the first model call of a task run, or of a chat
/// turn when `chat`.
async fn system_of(m: &FakeModel, chat: bool) -> String {
    for _ in 0..300 {
        let found = m.requests().into_iter().find(|c| magnus_scratch::system_of(c).contains("talking with the person you work for in a chat") == chat);
        if let Some(c) = found {
            return magnus_scratch::system_of(&c);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("no model call was made");
}

/// An appliance without web search or a sandbox reports that accurately.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_without_search_or_sandbox_is_not_told_it_has_them() {
    let m = FakeModel::start(script(|_| async move { Reply::Text("Noted.".into()) })).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    magnus_scratch::reconfigure(&mut app, |c| {
        c.remove("sandbox");
    })
    .await;
    let bob = app.api("bob");
    bob.submit("File the Q3 notes.").await;
    let system = system_of(&m, false).await;
    assert!(system.contains("# Runtime context") && system.contains("kill switch"), "{system}");
    assert!(system.contains("Public web search is unavailable"), "{system}");
    assert!(!system.contains("/workspace") && !system.contains("Browser tools are available"), "{system}");

    let (st, _) = bob.post("/v1/chat", json!({ "text": "Hello." })).await;
    assert_eq!(st.as_u16(), 201);
    let chat = system_of(&m, true).await;
    assert!(chat.contains("Public web search is unavailable"), "{chat}");
}

/// An appliance with both tells its runs they have them.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_with_search_and_sandbox_is_told_it_has_them() {
    let m = FakeModel::start(script(|_| async move { Reply::Text("Noted.".into()) })).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    magnus_scratch::reconfigure(&mut app, |c| {
        let mut t = toml::Table::new();
        t.insert("search_url".into(), "http://search.invalid".into());
        t.insert("allow_private_networks".into(), true.into());
        c.insert("web".into(), toml::Value::Table(t));
    })
    .await;
    app.api("bob").submit("File the Q3 notes.").await;
    let system = system_of(&m, false).await;
    assert!(system.contains("Public web search is available"), "{system}");
    assert!(system.contains("Isolated workspace: /workspace"), "{system}");
}
