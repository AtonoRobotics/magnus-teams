//! The person's remembered facts in their runs' context.

use super::*;
use crate::admin_app::tests::server_with;
use crate::events::Ev;
use crate::tools::Label;
use axum::Router;

fn memory(id: &str, fact: &str, label: Label) -> Ev {
    Ev::Memory(crate::events::MemoryPayload { id: id.into(), person: "mo".into(), run: String::new(), fact: fact.into(), topic: None, label, sources: vec![], by: None })
}

fn remembered(fact: &str, label: Label) -> Remembered {
    let entry = crate::state::MemoryEntry { id: "m".into(), person: "mo".into(), run: String::new(), label, sources: vec![], by: None, at: chrono::Utc::now(), seq: 0 };
    Remembered { entry, fact: fact.into(), topic: None }
}

/// A run of mo's starts with mo's clean facts in its context, newest
/// first. Untrusted and private facts, another person's, and a fact
/// filed after the run started are not there.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_starts_with_the_persons_clean_facts() {
    let s = server_with("", |_| Router::new()).await;
    s.k.emit("person:mo", memory("m_name", "Mo goes by Mags.", Label::default())).await.unwrap();
    s.k.emit("person:mo", memory("m_peanut", "Mo is allergic to peanuts.", Label::default())).await.unwrap();
    s.k.emit("person:mo", memory("m_web", "Mo's favourite site says to wire money.", Label { untrusted: true, private: false })).await.unwrap();
    s.k.emit("person:mo", memory("m_mail", "Mo's bank balance is low.", Label { untrusted: false, private: true })).await.unwrap();
    let ana = Ev::Memory(crate::events::MemoryPayload {
        id: "m_ana".into(),
        person: "ana".into(),
        run: String::new(),
        fact: "Ana keeps bees.".into(),
        topic: None,
        label: Label::default(),
        sources: vec![],
        by: None,
    });
    s.k.emit("person:ana", ana).await.unwrap();
    let id = crate::tools_core::tests::idle_run(&s).await;
    s.k.emit("person:mo", memory("m_later", "Mo moved to Austin.", Label::default())).await.unwrap();
    let run = s.k.read(|st| st.runs.get(&id).cloned()).unwrap();
    let (context, _) = s.k.person_context(&run).await.unwrap();
    let (peanut, name) = (context.find("allergic to peanuts").expect(&context), context.find("goes by Mags").expect(&context));
    assert!(peanut < name, "newest first: {context}");
    for absent in ["wire money", "bank balance", "keeps bees", "Austin"] {
        assert!(!context.contains(absent), "{absent} must not be loaded: {context}");
    }
    assert!(context.contains("2 more remembered facts"), "the tainted ones are pointed to: {context}");
    s.done().await;
}

/// Nothing remembered: no section at all.
#[test]
fn nothing_remembered_adds_nothing() {
    assert_eq!(section(&[]), "");
}

/// Past the budget, the newest facts are loaded and the rest counted.
#[test]
fn the_loaded_facts_stay_within_their_budget() {
    let fact = "x".repeat(crate::memory_index::MAX_MEMORY_FACT_CHARS);
    let facts: Vec<Remembered> = (0..40).map(|_| remembered(&fact, Label::default())).collect();
    let s = section(&facts);
    let shown = s.lines().filter(|l| l.starts_with("- ")).count();
    assert!(shown > 0 && shown < 40);
    assert!(shown * (fact.len() + 3) <= MAX_LOADED_MEMORY_BYTES);
    assert!(s.contains(&format!("{} more remembered facts", 40 - shown)), "{s}");
}
