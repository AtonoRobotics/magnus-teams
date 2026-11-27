//! Memory search: each person's current memory, searched where it is kept,
//! against a kernel booted on a fresh database as `magnus migrate` leaves it.

use super::tsquery;
use crate::admin_app::tests::{server_with, Server};
use crate::events::{Ev, MemoryArchivedPayload, MemorySupersededPayload};
use crate::tools::Label;
use axum::Router;

async fn boot() -> Server {
    server_with("", |_| Router::new()).await
}

fn memory(id: &str, person: &str, fact: &str, topic: Option<&str>) -> Ev {
    Ev::Memory(crate::events::MemoryPayload {
        id: id.into(),
        person: person.into(),
        run: String::new(),
        fact: fact.into(),
        topic: topic.map(String::from),
        label: Label::default(),
        sources: vec![],
        by: None,
    })
}

/// What `memory_search` serves mo for `query`, by memory id.
async fn served(s: &Server, query: &str) -> Vec<String> {
    let run = crate::tools_core::tests::idle_run(s).await;
    s.k.memory_hits(&run, query).await.unwrap().into_iter().map(|m| m.entry.id).collect()
}

#[test]
fn a_query_becomes_prefix_stems_of_its_words() {
    assert_eq!(tsquery("allergy schedules").as_deref(), Some("alle:* | schedu:*"));
    assert_eq!(tsquery("Émile's dog").as_deref(), Some("Émil:* | dog:*"), "case is left to the database, as for the text");
    assert_eq!(tsquery("is it my ok?"), None, "two-letter words are not searched");
    assert_eq!(tsquery("a & (b | !c) <-> d:*"), None, "no query syntax gets through");
    assert_eq!(tsquery("x'); DROP TABLE content; --").as_deref(), Some("DROP:* | TABL:* | cont:*"));
}

/// A word finds its other forms, in the fact or the topic, as soon as the
/// memory is saved; only the person's own memory is searched.
#[tokio::test(flavor = "multi_thread")]
async fn a_saved_memory_is_found_by_its_words_and_their_forms() {
    let s = boot().await;
    s.k.emit("person:mo", memory("m_allergy", "mo", "Mo is allergic to peanuts", None)).await.unwrap();
    s.k.emit("person:mo", memory("m_dock", "mo", "Deliveries come to door three", Some("scheduled deliveries"))).await.unwrap();
    s.k.emit("person:ana", memory("m_ana", "ana", "Ana is allergic to cats", None)).await.unwrap();
    assert_eq!(served(&s, "any allergy?").await, vec!["m_allergy".to_string()], "not ana's");
    assert_eq!(served(&s, "delivery schedules").await, vec!["m_dock".to_string()], "the topic is searched");
    assert!(served(&s, "submarine").await.is_empty());
    s.done().await;
}

/// Forgotten, trashed, archived and superseded memories are not served, and
/// a restored one is, at once: recall reads the state, there is no copy.
#[tokio::test(flavor = "multi_thread")]
async fn only_current_memory_is_served() {
    let s = boot().await;
    for (id, fact) in [("m_forget", "Acme forget note"), ("m_trash", "Acme gate code 4512"), ("m_rumour", "Acme might move"), ("m_lead_4", "Acme lead time is 4 weeks")] {
        s.k.emit("person:mo", memory(id, "mo", fact, None)).await.unwrap();
    }
    s.k.emit("person:mo", Ev::MemoryForgotten { id: "m_forget".into() }).await.unwrap();
    assert_eq!(s.k.trash("mo", false, "m_trash").await.unwrap(), Ok(()));
    s.k.emit("curator", Ev::MemoryArchived(MemoryArchivedPayload { id: "m_rumour".into(), person: "mo".into(), reason: "cold".into() })).await.unwrap();
    s.k.emit("person:mo", memory("m_lead_6", "mo", "Acme lead time is 6 weeks", None)).await.unwrap();
    s.k.emit(
        "curator",
        Ev::MemorySuperseded(MemorySupersededPayload {
            old_id: "m_lead_4".into(),
            new_id: "m_lead_6".into(),
            person: "mo".into(),
            reason: "newer".into(),
            valid_until: chrono::Utc::now(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(served(&s, "Acme").await, vec!["m_lead_6".to_string()]);

    assert!(s.k.restore("mo", "m_trash").await.unwrap());
    let mut found = served(&s, "Acme").await;
    found.sort();
    assert_eq!(found, vec!["m_lead_6".to_string(), "m_trash".to_string()], "a restored memory is found again");
    s.done().await;
}

/// No word to search for lists the newest memories, at most the cap.
#[tokio::test(flavor = "multi_thread")]
async fn no_words_lists_the_newest_memories() {
    let s = boot().await;
    let n = crate::recall::MAX_MEMORY_HITS + 2;
    for i in 0..n {
        s.k.emit("person:mo", memory(&format!("m_{i:02}"), "mo", &format!("fact number {i}"), None)).await.unwrap();
    }
    let listed = served(&s, "is it?").await;
    assert_eq!(listed.len(), crate::recall::MAX_MEMORY_HITS);
    assert_eq!(listed.first().map(String::as_str), Some(format!("m_{:02}", n - 1).as_str()), "newest first");
    assert!(!listed.contains(&"m_00".to_string()), "the oldest past the cap is left out");
    s.done().await;
}

/// The best match comes first, whatever its taint: an untrusted fact that
/// matches every word is not ranked under a clean one that matches one.
/// Its taint still reaches the run that reads it (`memory_taint`).
#[tokio::test(flavor = "multi_thread")]
async fn the_best_match_comes_first() {
    let s = boot().await;
    let mut best = memory("m_best", "mo", "Mo's dentist appointment is every March at Riverside dental", None);
    if let Ev::Memory(m) = &mut best {
        m.label = Label { untrusted: true, private: false };
    }
    s.k.emit("person:mo", best).await.unwrap();
    s.k.emit("person:mo", memory("m_weak", "mo", "Mo likes Riverside park", None)).await.unwrap();
    assert_eq!(served(&s, "Riverside dentist appointment March").await, vec!["m_best".to_string(), "m_weak".to_string()]);
    s.done().await;
}

/// A fact the person adds again in the app is the one already kept.
#[tokio::test(flavor = "multi_thread")]
async fn a_fact_the_person_adds_twice_is_kept_once() {
    let s = boot().await;
    let mo = s.token("mo").await;
    let (_, first) = s.call(&mo, reqwest::Method::POST, "/v1/memory", Some(serde_json::json!({ "fact": "Call me Mags." }))).await;
    let (_, again) = s.call(&mo, reqwest::Method::POST, "/v1/memory", Some(serde_json::json!({ "fact": "call me mags" }))).await;
    assert_eq!(again["id"], first["id"], "{again}");
    assert_eq!(again["already_remembered"], true);
    let (_, list) = s.get(&mo, "/v1/memory").await;
    assert_eq!(list["memory"].as_array().map(Vec::len), Some(1));
    s.done().await;
}
