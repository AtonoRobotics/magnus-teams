//! An app removed leaves nothing that names it: a different server added
//! later under the same name inherits no one's grants, rules or screened
//! tool definitions.

use crate::admin_app::tests::{server_with, Server};
use crate::events::Ev;
use axum::Router;
use serde_json::json;

async fn add(s: &Server, url: &str) {
    let ev: Ev = serde_json::from_value(json!({ "kind": "connector_added", "connector": { "name": "crm", "url": url, "audience": "internal", "label": "private" } })).unwrap();
    s.k.emit("person:sam", ev).await.unwrap();
}

/// mo's grant over crm and his rule for it, and a screened crm tool.
async fn named(s: &Server) {
    let grant: Ev = serde_json::from_value(json!({ "kind": "grant_set", "id": "g1", "person": "mo", "apps": ["crm"], "period": "day", "max_usd": 1.0, "cedar": "" })).unwrap();
    s.k.emit("person:mo", grant).await.unwrap();
    let compiled = crate::rules::compile("mo", "crm", "crm__send", &["ana@example.com".to_string()], false, crate::rules::Effect::Allow).unwrap();
    s.k.emit("person:mo", Ev::Rule(crate::rules::Change::Proposed { id: "r1".into(), person: "mo".into(), compiled: Some(compiled), words: None })).await.unwrap();
    s.k.emit("person:mo", Ev::Rule(crate::rules::Change::Accepted { id: "r1".into() })).await.unwrap();
    let screened = crate::tool_gate::Change::Screened {
        tool: "crm__send".into(),
        hash: "h".into(),
        description: "Send.".into(),
        input_schema: json!({}),
        verdict: crate::tool_gate::PASSED.into(),
        finding: String::new(),
        model: None,
        run: None,
    };
    s.k.emit("kernel", Ev::Tools(screened)).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_an_app_takes_away_everything_that_names_it() {
    let s = server_with("", |_| Router::new()).await;
    add(&s, "https://crm.example/mcp").await;
    named(&s).await;
    let r = s.k.remove_connector("person:sam", "crm").await.unwrap();
    assert_eq!(r["sign_ins_revoked"], 0);
    s.k.read(|st| {
        assert!(st.arrangements.grants()["g1"].revoked.is_some(), "mo's grant over the removed app stayed");
        assert!(st.rules.made_for("crm").is_empty(), "mo's always-allow rule for the removed app stayed");
        assert!(st.tools.pins.keys().all(|t| !t.starts_with("crm__")), "the removed app's screened definitions stayed");
    });
    // Another server under the same name starts with nothing of the old one's.
    add(&s, "https://other.example/mcp").await;
    assert!(s.k.tool_refusal("crm__send", "mo").is_some(), "the new server's tool inherited the old one's screen");
    s.done().await;
}
