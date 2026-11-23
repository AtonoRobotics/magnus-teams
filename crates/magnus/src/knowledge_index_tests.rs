//! The knowledge index follows the log, against a kernel booted on a fresh
//! database given the index's table.
use crate::admin_app::tests::{server_with, Server};
use crate::events::Ev;
use axum::Router;
use serde_json::Value;

/// The index's table and reader role, as `magnus migrate` makes them.
pub(crate) async fn with_index(s: &Server) {
    s.store().await.batch_execute(&crate::knowledge::migrate_sql("postgres")).await.unwrap();
}

pub(crate) fn entry(id: &str, text: &str, readers: Value) -> Ev {
    Ev::KnowledgeAdded {
        id: id.into(),
        title: format!("{id} title"),
        text: text.into(),
        readers: serde_json::from_value(readers).unwrap(),
        integrity: "untrusted".into(),
        source: Value::Null,
        author: "person:mo".into(),
        run: None,
        verification: None,
        value: None,
    }
}

/// The ids of the index's rows.
pub(crate) async fn rows(s: &Server) -> Vec<String> {
    s.store().await.query("SELECT id FROM public.knowledge ORDER BY id", &[]).await.unwrap().iter().map(|r| r.get(0)).collect()
}

async fn found(s: &Server, person: &str, query: &str) -> Vec<String> {
    let p = s.k.person(person).unwrap();
    s.k.knowledge_for(&p, query).await.unwrap().iter().filter_map(|e| e["id"].as_str().map(String::from)).collect()
}

/// With the index out of reach, an entry is trashed and removed all the
/// same; it is not found once the index is back, and the retry takes its
/// row, with its text, out.
#[tokio::test(flavor = "multi_thread")]
async fn an_entry_trashed_and_removed_while_the_index_is_down_leaves_it() {
    let s = server_with("", |_| Router::new()).await;
    with_index(&s).await;
    s.k.emit("person:mo", entry("kb_pump", "the pump's spare is in bay four", serde_json::json!([["person:mo"]]))).await.unwrap();
    assert_eq!(found(&s, "mo", "pump spare").await, vec!["kb_pump".to_string()], "a filed entry was not indexed at once");

    let db = s.store().await;
    db.batch_execute("ALTER TABLE public.knowledge RENAME TO knowledge_away").await.unwrap();
    assert_eq!(s.k.trash("mo", false, "kb_pump").await.unwrap(), Ok(()), "trashing waited on the index");
    s.k.remove("person:mo", "kb_pump").await.unwrap();
    assert!(s.k.knowledge.behind_on("kb_pump").is_some(), "the index was not marked behind on the entry");

    db.batch_execute("ALTER TABLE public.knowledge_away RENAME TO knowledge").await.unwrap();
    assert!(found(&s, "mo", "pump spare").await.is_empty(), "a removed entry was found");
    s.k.catch_up_index(true).await.unwrap();
    assert!(rows(&s).await.is_empty(), "the removed entry's row, with its text, stayed in the index");
    assert!(s.k.knowledge.behind_on("kb_pump").is_none());
    s.done().await;
}

/// An entry put back from the trash is found again.
#[tokio::test(flavor = "multi_thread")]
async fn a_restored_entry_is_indexed_again() {
    let s = server_with("", |_| Router::new()).await;
    with_index(&s).await;
    s.k.emit("person:mo", entry("kb_valve", "the valve opens anticlockwise", serde_json::json!([["person:mo"]]))).await.unwrap();
    assert_eq!(s.k.trash("mo", false, "kb_valve").await.unwrap(), Ok(()));
    assert!(rows(&s).await.is_empty(), "a trashed entry stayed in the index");
    assert!(s.k.restore("mo", "kb_valve").await.unwrap());
    assert_eq!(found(&s, "mo", "valve").await, vec!["kb_valve".to_string()]);
    s.done().await;
}
