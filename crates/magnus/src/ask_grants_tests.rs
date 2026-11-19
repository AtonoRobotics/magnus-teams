use crate::admin_app::tests::server_with;
use crate::events::Ev;
use crate::inbox::Answer;
use axum::Router;
use serde_json::json;

/// An ask Auto Review raised for mo's action in an app, waiting.
async fn ask(k: &crate::kernel::Kernel, id: &str, risk: &str) {
    let details = json!({
        "call_id": "c1", "tool": "github__merge_pull_request", "input": { "pr": 7 }, "input_hash": "h1",
        "risk": risk, "app": "github", "app_title": "GitHub", "audience": "internal",
        "ask_destinations": [], "reason": "outside_request", "why": "beyond the request", "rule": crate::asks::OUTSIDE_REQUEST,
    });
    let ev = Ev::Item {
        id: id.into(),
        item_kind: "approval".into(),
        task: Some("t1".into()),
        run: None,
        person: "mo".into(),
        summary: "Magnus wants to merge pull request in GitHub.".into(),
        details,
    };
    k.emit("kernel", ev).await.unwrap();
}

/// Handle on an ask sets mo's grant from the form and allows the rest
/// of the task in the same decision; it is not offered where Allow
/// always is, and no one else answers it.
#[tokio::test]
async fn handling_an_ask_sets_the_grant_and_allows_the_task() {
    let s = server_with("", |_| Router::new()).await;
    let mo = s.k.person("mo").unwrap();
    let sam = s.k.person("sam").unwrap();
    ask(&s.k, "item_h", "irreversible").await;
    let grant = json!({ "title": "Merge my pull requests", "apps": ["github"], "allow": ["irreversible"], "period": "week", "max_usd": 2.0 });
    let body = json!({ "decision": "approve", "scope": "handle", "grant": grant });

    // The card offers it, with the form's starting values.
    let mut rows = s.k.read(|st| vec![crate::inbox::render(st, &mo, &st.inbox.items()["item_h"])]);
    assert!(rows[0]["ask"]["offers"].as_array().unwrap().contains(&json!("handle")), "{}", rows[0]["ask"]);
    s.k.handle_prefill("mo", &mut rows).await;
    let h = &rows[0]["ask"]["handle"];
    assert_eq!((h["apps"].clone(), h["allow"].clone()), (json!(["github"]), json!(["irreversible", "send"])), "{h}");
    assert!(h["your_apps"].as_array().unwrap().iter().any(|a| a["name"] == "github"), "{h}");

    assert!(matches!(s.k.respond(&sam, "item_h", &body).await.unwrap(), Answer::NotFound | Answer::Forbidden(_)), "someone else handled mo's ask");
    let half = json!({ "decision": "approve", "scope": "handle", "grant": { "title": "x" } });
    assert!(matches!(s.k.respond(&mo, "item_h", &half).await.unwrap(), Answer::Invalid(_)));
    assert!(s.k.read(|st| st.arrangements.grants().is_empty()), "a refused Handle set a grant");

    assert!(matches!(s.k.respond(&mo, "item_h", &body).await.unwrap(), Answer::Ok));
    let (grants, approvals, resolved) =
        s.k.read(|st| (st.arrangements.grants().values().cloned().collect::<Vec<_>>(), st.inbox.approvals().clone(), st.inbox.items()["item_h"].resolution.clone()));
    assert_eq!(grants.len(), 1);
    assert_eq!((grants[0].person.as_str(), grants[0].apps.clone(), grants[0].allow.clone()), ("mo", vec!["github".to_string()], vec!["irreversible".to_string()]));
    let ap = approvals.iter().find(|a| a.task == "t1" && a.tool == "github__merge_pull_request").expect("no approval for the handled ask");
    assert_eq!((ap.scope.as_str(), ap.by.as_str()), ("task", "person:mo"), "Handle did not allow the rest of the task");
    assert_eq!(resolved.map(|r| (r.0, r.1)), Some(("approve".to_string(), Some("handle".to_string()))));

    // Where Allow always is offered (what can be undone), Handle is not.
    ask(&s.k, "item_r", "reversible_write").await;
    let row = s.k.read(|st| crate::inbox::render(st, &mo, &st.inbox.items()["item_r"]));
    assert!(!row["ask"]["offers"].as_array().unwrap().contains(&json!("handle")), "{}", row["ask"]);
    assert!(matches!(s.k.respond(&mo, "item_r", &body).await.unwrap(), Answer::Invalid(_)), "Handle was taken where it is not offered");
    s.done().await;
}
