//! Whose a knowledge entry and a lesson are, by the Rust rule, the SQL rule
//! and an erasure, against a kernel booted on a fresh database.
use super::*;
use crate::admin_app::tests::{server_with, Server};
use crate::events::Ev;
use crate::knowledge_index::tests::{entry, rows, with_index};
use crate::tools::Label;
use axum::Router;
use serde_json::json;
use std::collections::BTreeMap;

fn clauses(v: serde_json::Value) -> Vec<Vec<String>> {
    serde_json::from_value(v).unwrap()
}

#[test]
fn an_entry_only_one_person_may_read_is_theirs() {
    for (readers, owner) in [
        (json!([["person:mo"]]), Some("mo")),
        (json!([["group:staff"], ["person:mo"]]), Some("mo")),
        (json!([["person:mo"], ["person:mo"]]), Some("mo")),
        (json!([["person:mo", "group:staff"]]), None),
        (json!([["person:mo", "person:sam"]]), None),
        (json!([["person:mo"], ["person:sam"]]), None),
        (json!([["group:staff"]]), None),
        (json!([]), None),
    ] {
        assert_eq!(entry_owner(&clauses(readers.clone())), owner, "{readers}");
    }
}

fn lesson(item: &str, person: &str, text: &str) -> Ev {
    Ev::Playbook(crate::events::PlaybookPayload {
        item: item.into(),
        op: "add".into(),
        person: person.into(),
        run: "run_x".into(),
        text: Some(text.into()),
        addresses: Some("a failure".into()),
        label: Label::default(),
    })
}

/// Whose each content row says it is.
async fn owners(s: &Server) -> BTreeMap<i64, Option<String>> {
    s.store().await.query("SELECT seq, person FROM public.content", &[]).await.unwrap().iter().map(|r| (r.get(0), r.get(1))).collect()
}

/// The rows the log wrote and the rows `migrate` fills give every entry
/// and lesson the same owner, an entry shared since by its readers now.
#[tokio::test(flavor = "multi_thread")]
async fn the_rust_and_sql_rules_give_entries_and_lessons_the_same_owner() {
    let s = server_with("", |_| Router::new()).await;
    let mut want = BTreeMap::new();
    for (ev, who) in [
        (entry("kb_a", "a", json!([["person:mo"]])), Some("mo")),
        (entry("kb_b", "b", json!([["group:staff"]])), None),
        (entry("kb_c", "c", json!([["group:staff"], ["person:mo"]])), Some("mo")),
        (entry("kb_d", "d", json!([])), None),
        (entry("kb_e", "e", json!([["person:mo"]])), None),
        (lesson("pb_a", "mo", "check twice"), Some("mo")),
    ] {
        let e = s.k.emit("person:mo", ev).await.unwrap();
        want.insert(e.seq, who.map(String::from));
    }
    s.k.emit("person:mo", Ev::KnowledgeShared { id: "kb_e".into(), readers: clauses(json!([["group:staff", "person:mo"]])), by: "mo".into() }).await.unwrap();
    let db = s.store().await;
    db.batch_execute("UPDATE public.content SET person = NULL").await.unwrap();
    db.batch_execute(crate::log::CONTENT_OWNERS).await.unwrap();
    db.batch_execute(&owners_sql()).await.unwrap();
    let sql = owners(&s).await;
    for (seq, who) in &want {
        assert_eq!(sql.get(seq), Some(who), "event {seq}");
    }
    s.done().await;
}

/// Erased, mo's own entry and lesson leave the content store, the state,
/// the knowledge index and the backups; an entry of theirs shared with a
/// group stays for the group.
#[tokio::test(flavor = "multi_thread")]
async fn an_erasure_takes_a_persons_own_entries_and_lessons() {
    let s = server_with("", |_| Router::new()).await;
    with_index(&s).await;
    let own = s.k.emit("person:mo", entry("kb_own", "mo keeps a spare key under the mat", json!([["person:mo"]]))).await.unwrap().seq;
    let shared = s.k.emit("person:mo", entry("kb_shared", "the office opens at eight", json!([["person:mo"]]))).await.unwrap().seq;
    s.k.emit("person:mo", Ev::KnowledgeShared { id: "kb_shared".into(), readers: clauses(json!([["group:staff", "person:mo"]])), by: "mo".into() }).await.unwrap();
    let taught = s.k.emit("person:mo", lesson("pb_mo", "mo", "mo hates morning calls")).await.unwrap().seq;
    let before = owners(&s).await;
    assert_eq!((before[&own].as_deref(), before[&taught].as_deref()), (Some("mo"), Some("mo")), "the rows were not written as mo's");
    assert_eq!(rows(&s).await, vec!["kb_own".to_string(), "kb_shared".to_string()]);
    let made = s.k.back_up("person:sam").await.unwrap();
    let file = s.k.cfg.backup_dir().join(made["file"].as_str().unwrap());

    s.k.erase_person("person:sam", "mo").await.unwrap();

    let after = owners(&s).await;
    assert!(!after.contains_key(&own) && !after.contains_key(&taught), "mo's entry or lesson kept its content row");
    assert!(after.contains_key(&shared), "the entry mo shared with a group lost its content");
    let (entries, lessons) = s.k.read(|st| (st.library.knowledge().keys().cloned().collect::<Vec<_>>(), st.library.playbook().len()));
    assert_eq!((entries, lessons), (vec!["kb_shared".to_string()], 0), "the state kept mo's entry or lesson");
    assert_eq!(rows(&s).await, vec!["kb_shared".to_string()], "mo's entry stayed in the knowledge index");
    let kept = crate::backup::read(&s.k.cfg, &file).unwrap().to_string();
    for words in ["spare key under the mat", "hates morning calls"] {
        assert!(!kept.contains(words), "the backup still holds {words:?}");
    }
    assert!(kept.contains("the office opens at eight"), "the backup lost the shared entry");
    s.done().await;
}
