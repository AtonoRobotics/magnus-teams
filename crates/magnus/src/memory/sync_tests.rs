//! Recall's store follows people's memory as each write lands, against a
//! kernel booted on a fresh database as `magnus migrate` leaves it.

use crate::admin_app::tests::{server_with, Server};
use crate::events::{Ev, MemoryArchivedPayload, MemorySupersededPayload};
use crate::memory::bridge::retrieve_for_person;
use crate::tools::Label;
use axum::Router;

async fn boot() -> Server {
    server_with("[memory]\nembedding_dim = 4\n", |_| Router::new()).await
}

fn memory(id: &str, person: &str, fact: &str) -> Ev {
    Ev::Memory { id: id.into(), person: person.into(), run: String::new(), fact: fact.into(), topic: None, label: Label::default(), sources: vec![], by: None }
}

async fn sources(s: &Server, person: &str) -> Vec<String> {
    s.store()
        .await
        .query("SELECT DISTINCT source_id FROM public.memory_sources WHERE owner_id = $1 ORDER BY source_id", &[&person])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect()
}

/// What the store finds for `query`, by memory id.
async fn in_store(s: &Server, person: &str, query: &str) -> Vec<String> {
    let hits = retrieve_for_person(&s.k.cfg.database.url, &s.k.cfg.memory, person, query, true).await.unwrap();
    hits.into_iter().map(|m| m.entry.sources[0].split('@').next().unwrap_or_default().to_string()).collect()
}

/// What `memory_search` serves mo for `query`, by memory id.
async fn served(s: &Server, query: &str) -> Vec<String> {
    let run = crate::tools_core::tests::idle_run(s).await;
    s.k.memory_hits(&run, query).await.unwrap().into_iter().map(|m| m.entry.id).collect()
}

/// A saved memory is found by the very next search.
#[tokio::test(flavor = "multi_thread")]
async fn a_saved_memory_is_found_at_once() {
    let s = boot().await;
    s.k.emit("person:mo", memory("m_pump", "mo", "The pump spare is in bay four")).await.unwrap();
    assert_eq!(served(&s, "pump spares").await, vec!["m_pump".to_string()]);
    assert_eq!(served(&s, "").await, vec!["m_pump".to_string()], "an empty search lists what is remembered");
    s.done().await;
}

/// Forgetting, trashing, archiving and superseding each take the copy out of
/// the store as the write lands; restoring puts it back.
#[tokio::test(flavor = "multi_thread")]
async fn each_change_reaches_the_store_with_its_write() {
    let s = boot().await;
    for (id, fact) in [("m_forget", "Forget me"), ("m_trash", "The gate code is 4512"), ("m_rumour", "Acme might move"), ("m_lead_4", "Acme lead time is 4 weeks")] {
        s.k.emit("person:mo", memory(id, "mo", fact)).await.unwrap();
    }
    assert_eq!(sources(&s, "mo").await.len(), 4);

    s.k.emit("person:mo", Ev::MemoryForgotten { id: "m_forget".into() }).await.unwrap();
    assert_eq!(s.k.trash("mo", false, "m_trash").await.unwrap(), Ok(()));
    s.k.emit("curator", Ev::MemoryArchived(MemoryArchivedPayload { id: "m_rumour".into(), person: "mo".into(), reason: "cold".into() })).await.unwrap();
    s.k.emit("person:mo", memory("m_lead_6", "mo", "Acme lead time is 6 weeks")).await.unwrap();
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
    assert_eq!(sources(&s, "mo").await, vec!["m_lead_6".to_string()]);
    assert_eq!(served(&s, "Acme lead time").await, vec!["m_lead_6".to_string()]);

    assert!(s.k.restore("mo", "m_trash").await.unwrap());
    assert_eq!(sources(&s, "mo").await, vec!["m_lead_6".to_string(), "m_trash".to_string()], "a restored memory was not copied back");
    s.done().await;
}

/// At boot the store is brought level with the state: memory from before the
/// store is copied in, and a copy of nothing current is deleted.
#[tokio::test(flavor = "multi_thread")]
async fn boot_brings_the_store_level() {
    let s = boot().await;
    s.k.emit("person:mo", memory("m_history", "mo", "The forklift battery is charged on Fridays")).await.unwrap();
    let db = s.store().await;
    // As if saved before the store existed, and a stale copy of a memory gone.
    db.batch_execute("DELETE FROM public.memory_enrichment_jobs; DELETE FROM public.memory_sources").await.unwrap();
    db.execute(
        "INSERT INTO public.memory_sources (tenant_id, source_id, rev, owner_id, readers, untrusted, private, content_hash, text, tsv, occurred_at)
         VALUES ('default', 'm_gone', 1, 'ana', '[[\"user:ana\"]]', FALSE, TRUE, 'h', 'gone', to_tsvector('simple', 'gone'), now())",
        &[],
    )
    .await
    .unwrap();
    s.k.mirror_all_memory().await.unwrap();
    assert_eq!(sources(&s, "mo").await, vec!["m_history".to_string()]);
    assert!(sources(&s, "ana").await.is_empty(), "a copy of nothing current stayed");
    s.done().await;
}

/// A copy left in the store for a memory no longer current is never served.
#[tokio::test(flavor = "multi_thread")]
async fn a_stale_copy_is_never_served() {
    let s = boot().await;
    s.k.emit("person:mo", memory("m_secret", "mo", "The safe code is 7731")).await.unwrap();
    s.k.emit("person:mo", Ev::MemoryForgotten { id: "m_secret".into() }).await.unwrap();
    s.store()
        .await
        .execute(
            "INSERT INTO public.memory_sources (tenant_id, source_id, rev, owner_id, readers, untrusted, private, content_hash, text, tsv, occurred_at)
             VALUES ('default', 'm_secret', 1, 'mo', '[[\"user:mo\"]]', FALSE, TRUE, 'h', 'The safe code is 7731', to_tsvector('simple', 'The safe code is 7731'), now())",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(in_store(&s, "mo", "safe code").await, vec!["m_secret".to_string()], "the planted copy should be in the store");
    assert!(served(&s, "safe code").await.is_empty(), "a forgotten memory was served from a stale copy");
    s.done().await;
}

/// A store that cannot take the copy fails the save, instead of leaving
/// recall silently behind.
#[tokio::test(flavor = "multi_thread")]
async fn a_store_failure_fails_the_save() {
    let s = boot().await;
    s.store().await.batch_execute("ALTER TABLE public.memory_enrichment_jobs RENAME TO memory_enrichment_jobs_away").await.unwrap();
    assert!(s.k.emit("person:mo", memory("m_lost", "mo", "Saved with the store broken")).await.is_err());
    s.done().await;
}
