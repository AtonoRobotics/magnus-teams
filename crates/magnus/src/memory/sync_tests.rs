//! The unified memory store follows people's memory through `memory::sync`,
//! against a kernel booted on a fresh database given the memory tables.

use crate::admin_app::tests::{server_with, Server};
use crate::events::{Ev, MemoryArchivedPayload, MemorySupersededPayload};
use crate::log::Event;
use crate::memory::bridge::{retrieve_for_person, AccessTracking};
use crate::memory::schema::migrate_if_enabled;
use crate::memory::sync::MemorySync;
use crate::tools::Label;
use axum::Router;

const V2_FOR_MO: &str = "[memory]\nembedding_dim = 4\n[memory.person_modes]\nmo = \"v2\"\n";
const LEGACY_MO_V2_ANA: &str = "[memory]\nembedding_dim = 4\n[memory.person_modes]\nana = \"v2\"\n";

async fn boot(config: &str) -> Server {
    let s = server_with(config, |_| Router::new()).await;
    // Copies read the person's content the way recall does, behind the walls.
    s.store().await.batch_execute(&crate::log::content_walls_sql("postgres")).await.unwrap();
    let url = s.k.cfg.database.url.clone();
    migrate_if_enabled(&url, &url, &s.k.cfg.memory).await.unwrap();
    s.k.memory_sync.mark_everything();
    s
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

/// The memory ids recalled; a not-yet-enriched source is named `id@rev`.
async fn recalled(s: &Server, person: &str, query: &str) -> Vec<String> {
    let hits = retrieve_for_person(&s.k.cfg.database.url, &s.k.cfg.memory, person, query, true, AccessTracking::Skip).await.unwrap();
    hits.into_iter().map(|m| m.entry.id.split('@').next().unwrap_or_default().to_string()).collect()
}

/// History and a new save arrive in either order; after one pass the store
/// holds all of it, and only then does it serve the person.
#[tokio::test(flavor = "multi_thread")]
async fn history_and_a_save_before_the_first_search_are_both_synced() {
    let s = boot(V2_FOR_MO).await;
    s.k.emit("person:mo", memory("m_old", "mo", "The pump spare is in bay four")).await.unwrap();
    s.k.sync_memory().await.unwrap();
    s.k.emit("person:mo", memory("m_new", "mo", "The forklift battery is charged on Fridays")).await.unwrap();
    s.k.sync_memory().await.unwrap();
    assert!(s.k.memory_sync.is_synced("mo"));
    assert_eq!(sources(&s, "mo").await, vec!["m_new".to_string(), "m_old".to_string()]);
    assert_eq!(recalled(&s, "mo", "pump spare").await, vec!["m_old".to_string()]);
    s.done().await;
}

/// A memory forgotten while its owner is on Legacy still leaves the store,
/// so it cannot come back when they are moved to V2 again.
#[tokio::test(flavor = "multi_thread")]
async fn a_forget_while_the_person_is_on_legacy_still_deletes_the_copy() {
    let s = boot(LEGACY_MO_V2_ANA).await;
    s.k.emit("person:mo", memory("m_secret", "mo", "Mo's private note")).await.unwrap();
    // Left over from an earlier V2 period: a copy written while mo was on V2.
    s.store()
        .await
        .execute(
            "INSERT INTO public.memory_sources (tenant_id, source_id, rev, owner_id, readers, untrusted, private, content_hash, text, tsv, occurred_at)
             VALUES ('default', 'm_secret', 1, 'mo', '[[\"user:mo\"]]', FALSE, TRUE, 'h', 'Mo''s private note', to_tsvector('simple', 'Mo''s private note'), now())",
            &[],
        )
        .await
        .unwrap();
    s.k.sync_memory().await.unwrap();
    assert_eq!(sources(&s, "mo").await, vec!["m_secret".to_string()], "a Legacy person's current memory is not deleted");

    let id = "m_secret".to_string();
    s.k.emit("person:mo", Ev::MemoryForgotten { id }).await.unwrap();
    s.k.sync_memory().await.unwrap();
    assert!(sources(&s, "mo").await.is_empty(), "the forgotten memory's copy stayed in the store");
    s.done().await;
}

/// Archived and superseded memories are no longer current, so they leave
/// the store and V2 cannot serve them.
#[tokio::test(flavor = "multi_thread")]
async fn archived_and_superseded_memories_leave_the_store() {
    let s = boot(V2_FOR_MO).await;
    s.k.emit("person:mo", memory("m_lead_4", "mo", "Acme lead time is 4 weeks")).await.unwrap();
    s.k.emit("person:mo", memory("m_rumour", "mo", "Acme might move offices")).await.unwrap();
    s.k.sync_memory().await.unwrap();
    assert_eq!(sources(&s, "mo").await.len(), 2);

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
    s.k.emit("curator", Ev::MemoryArchived(MemoryArchivedPayload { id: "m_rumour".into(), person: "mo".into(), reason: "cold".into() })).await.unwrap();
    s.k.sync_memory().await.unwrap();
    assert_eq!(sources(&s, "mo").await, vec!["m_lead_6".to_string()]);
    assert_eq!(recalled(&s, "mo", "Acme lead time").await, vec!["m_lead_6".to_string()]);
    s.done().await;
}

/// With the store failing, a save is not held up, the person is not served
/// from the store, and the next pass copies what was missed.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_copy_does_not_hold_up_the_save_and_is_retried() {
    let s = boot(V2_FOR_MO).await;
    s.k.sync_memory().await.unwrap();
    let db = s.store().await;
    // Every copy fails: ingest cannot queue its enrichment job.
    db.batch_execute("ALTER TABLE public.memory_enrichment_jobs RENAME TO memory_enrichment_jobs_away").await.unwrap();

    let started = std::time::Instant::now();
    s.k.emit("person:mo", memory("m_during", "mo", "The dock door code changed")).await.unwrap();
    assert!(started.elapsed() < std::time::Duration::from_millis(500), "the save waited on the unified store");
    assert!(s.k.sync_memory().await.is_err(), "the copy could not be made");
    assert!(s.k.memory_sync.behind(), "a failed pass keeps its marks");
    assert!(!s.k.memory_sync.is_synced("mo"));

    db.batch_execute("ALTER TABLE public.memory_enrichment_jobs_away RENAME TO memory_enrichment_jobs").await.unwrap();
    s.k.sync_memory().await.unwrap();
    assert!(s.k.memory_sync.is_synced("mo"));
    assert_eq!(sources(&s, "mo").await, vec!["m_during".to_string()]);
    s.done().await;
}

fn event(ev: Ev) -> Event {
    Event { seq: 1, at: chrono::Utc::now(), actor: "person:mo".into(), ev }
}

#[test]
fn nobody_is_in_step_until_a_pass_and_a_write_or_forget_takes_them_out() {
    let sync = MemorySync::new();
    assert!(!sync.is_synced("mo"), "everyone is behind at start");
    sync.take_for_test();
    sync.settle_for_test("mo");
    assert!(sync.is_synced("mo"));

    sync.follow(&[event(memory("m1", "mo", "a fact"))]);
    assert!(!sync.is_synced("mo"), "a save takes the person out of step");
    sync.take_for_test();
    sync.settle_for_test("mo");
    assert!(sync.is_synced("mo"));

    sync.follow(&[event(Ev::MemoryForgotten { id: "m1".into() })]);
    assert!(!sync.is_synced("mo"), "a pending forget takes everyone out of step");
    sync.take_for_test();
    assert!(sync.is_synced("mo"));
}

#[test]
fn a_write_during_a_pass_keeps_the_person_out_of_step() {
    let sync = MemorySync::new();
    sync.take_for_test();
    // The pass is working on mo when another save for mo lands.
    sync.follow(&[event(memory("m2", "mo", "another fact"))]);
    sync.settle_for_test("mo");
    assert!(!sync.is_synced("mo"), "the pass finished before the new save was copied");
}
