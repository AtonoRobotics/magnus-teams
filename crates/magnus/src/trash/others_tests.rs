//! The owner moderates knowledge shared with groups, and never deletes an
//! entry only its person reads. sam owns the server, mo is a member.
use crate::admin_app::tests::server_with;
use crate::events::Ev;
use axum::Router;
use serde_json::{json, Value};

fn entry(id: &str, readers: Value) -> Ev {
    Ev::KnowledgeAdded {
        id: id.into(),
        title: format!("{id} title"),
        text: "what mo knows".into(),
        readers: serde_json::from_value(readers).unwrap(),
        integrity: "trusted".into(),
        source: Value::Null,
        author: "person:mo".into(),
        run: None,
        verification: None,
        value: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_owner_does_not_delete_a_persons_own_knowledge() {
    let s = server_with("", |_| Router::new()).await;
    s.k.emit("person:mo", entry("kb_own", json!([["person:mo"]]))).await.unwrap();
    s.k.emit("person:mo", entry("kb_team", json!([["group:staff"]]))).await.unwrap();
    let sam = s.token("sam").await;
    let (st, v) = s.post(&sam, "/v1/trash", json!({ "item": "kb_own" })).await;
    assert_eq!(st, 404, "the owner trashed mo's own entry: {v}");
    assert!(s.k.read(|st| st.knowledge.contains_key("kb_own")), "mo's entry is gone");
    assert!(!s.get(&sam, "/v1/trash").await.1.to_string().contains("kb_own title"), "mo's entry's title reached the owner's trash");
    // What is shared with a group stays the owner's to moderate, and mo's
    // own stays mo's to delete (the move itself needs the knowledge index,
    // which this server has not got, so only the rule is asked).
    let st = s.k.read(|st| (st.may_trash("sam", true, "kb_team").is_ok(), st.may_trash("mo", false, "kb_own").is_ok(), st.may_trash("sam", true, "kb_own").is_ok()));
    assert_eq!(st, (true, true, false));
    s.done().await;
}
