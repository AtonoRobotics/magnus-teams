//! Each run is told its harness as it actually is (#270): what wakes it,
//! what it keeps, what bounds it, and only the capabilities this appliance
//! offers it — never web search or a sandbox it does not have.

use magnus_invariants::*;
use serde_json::json;

/// The system prompt of the first model call of a task run, or of a chat
/// turn when `chat`.
async fn system_of(m: &FakeModel, chat: bool) -> String {
    for _ in 0..300 {
        let found = m.requests().into_iter().find(|c| {
            let s = c.body["system"].as_str().unwrap_or_default();
            s.contains("talking with the person you work for in a chat") == chat
        });
        if let Some(c) = found {
            return c.body["system"].as_str().unwrap_or_default().to_string();
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("no model call was made");
}

/// An appliance without web search or a sandbox tells neither run it has
/// them, and still says what wakes it, keeps and bounds it.
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
    assert!(system.contains("# Your harness"), "{system}");
    assert!(system.contains("You wake when they message you"), "{system}");
    assert!(system.contains("You keep:"), "{system}");
    assert!(system.contains("kill switch"), "{system}");
    assert!(system.contains("You cannot search the web"), "{system}");
    assert!(!system.contains("You can search the web"), "{system}");
    assert!(!system.contains("fresh VM"), "{system}");
    assert!(!system.contains("browser tools"), "{system}");

    let (st, _) = bob.post("/v1/chat", json!({ "text": "Hello." })).await;
    assert_eq!(st.as_u16(), 201);
    let chat = system_of(&m, true).await;
    assert!(!chat.contains("search the web, fetch a page"), "{chat}");
    assert!(chat.contains("when your harness searches it"), "{chat}");
    assert!(chat.contains("You cannot search the web"), "{chat}");
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
    assert!(system.contains("You can search the web with web_search"), "{system}");
    assert!(system.contains("fresh VM"), "{system}");
    assert!(!system.contains("You cannot search the web"), "{system}");
}
