//! A secret a person types into a card (an answer to the agent's question,
//! a reason for not allowing an action, an endorsement's note) is masked as
//! it arrives: neither the log nor what the model is told ever holds it.

use crate::admin_app::tests::{server_with, Server};
use crate::config::Person;
use crate::events::Ev;
use crate::kernel::NewRun;
use axum::Router;
use serde_json::json;

const KEY: &str = "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWx";

fn person(id: &str, roles: &[&str]) -> Person {
    Person { id: id.into(), name: id.into(), token: None, groups: vec![], roles: roles.iter().map(|r| r.to_string()).collect() }
}

/// Every event body and every piece of content the log holds.
async fn logged(s: &Server) -> String {
    let db = s.store().await;
    let rows = db.query("SELECT body::text FROM public.events UNION ALL SELECT body::text FROM public.content", &[]).await.unwrap();
    rows.iter().map(|r| r.get::<_, String>(0)).collect::<Vec<_>>().join("\n")
}

/// A card of `kind` on a run of mo's, waiting on tool call `c1`.
async fn card(s: &Server, kind: &str) -> String {
    let (_, run) = s.k.create_run("person:mo", "mo", "take a note".into(), vec![], NewRun::default()).await.unwrap();
    let r = s.k.read(|st| st.runs[&run].clone());
    s.k.item(&r, kind, "Which key should I use?".into(), json!({ "call_id": "c1" })).await.unwrap()
}

fn told(s: &Server, item: &str) -> String {
    let run = s.k.read(|st| st.inbox.items()[item].run.clone().unwrap());
    s.k.read(|st| format!("{:?}", st.runs[&run].results.get("c1")))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_typed_as_an_answer_is_masked_in_the_log_and_for_the_model() {
    let s = server_with("", |_| Router::new()).await;
    let item = card(&s, "question").await;
    s.k.respond(&person("mo", &[]), &item, &json!({ "decision": "answer", "text": format!("use {KEY}") })).await.unwrap();
    let model = told(&s, &item);
    assert!(model.contains("[hidden API key]") && !model.contains(KEY), "the model was told: {model}");
    let log = logged(&s).await;
    assert!(!log.contains(KEY), "the answer's key reached the log");
    assert!(log.contains("The person answered: use [hidden API key]"));
    s.done().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_typed_as_a_reason_for_not_allowing_is_masked() {
    let s = server_with("", |_| Router::new()).await;
    let item = card(&s, "approval").await;
    s.k.respond(&person("mo", &[]), &item, &json!({ "decision": "reject", "text": format!("no, and my key is {KEY}") })).await.unwrap();
    let model = told(&s, &item);
    assert!(model.contains("[hidden API key]") && !model.contains(KEY), "the model was told: {model}");
    assert!(!logged(&s).await.contains(KEY), "the reason's key reached the log");
    s.done().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_typed_as_an_endorsements_note_is_masked() {
    let s = server_with("", |_| Router::new()).await;
    let entry = Ev::KnowledgeAdded {
        id: "kb_1".into(),
        title: "Billing".into(),
        text: "Invoices go out on the 1st.".into(),
        readers: vec![vec!["person:sam".into()]],
        integrity: "trusted".into(),
        source: serde_json::Value::Null,
        author: "person:sam".into(),
        run: None,
        verification: None,
        value: None,
    };
    s.k.emit("person:sam", entry).await.unwrap();
    let item = "item_endorse".to_string();
    let card = Ev::Item {
        id: item.clone(),
        item_kind: "endorsement".into(),
        task: None,
        run: None,
        person: "sam".into(),
        summary: "Make this a company fact? Billing".into(),
        details: json!({ "entry": "kb_1", "readers": [["person:sam"]], "requested_by": "sam" }),
    };
    s.k.emit("person:sam", card).await.unwrap();
    // The entry is indexed after; this server has no knowledge index, so
    // only what was written is asked.
    let _ = s.k.respond(&person("sam", &["owner"]), &item, &json!({ "decision": "endorse", "text": format!("checked with {KEY}") })).await;
    let log = logged(&s).await;
    assert!(log.contains("knowledge_endorsed"), "the endorsement was not written");
    assert!(!log.contains(KEY), "the note's key reached the log");
    s.done().await;
}
