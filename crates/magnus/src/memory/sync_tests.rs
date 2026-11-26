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

/// With the store failing, the person is not served from it, and the next
/// pass copies what was missed.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_copy_is_retried() {
    let s = boot(V2_FOR_MO).await;
    s.k.sync_memory().await.unwrap();
    let db = s.store().await;
    // Every copy fails: ingest cannot queue its enrichment job.
    db.batch_execute("ALTER TABLE public.memory_enrichment_jobs RENAME TO memory_enrichment_jobs_away").await.unwrap();

    s.k.emit("person:mo", memory("m_during", "mo", "The dock door code changed")).await.unwrap();
    assert!(s.k.sync_memory().await.is_err(), "the copy could not be made");
    assert!(s.k.memory_sync.behind(), "a failed pass keeps its marks");
    assert!(s.k.memory_sync.failure().is_some(), "the failure is kept to be shown");
    assert!(!s.k.memory_sync.is_synced("mo"));

    db.batch_execute("ALTER TABLE public.memory_enrichment_jobs_away RENAME TO memory_enrichment_jobs").await.unwrap();
    s.k.sync_memory().await.unwrap();
    assert!(s.k.memory_sync.is_synced("mo"));
    assert!(s.k.memory_sync.failure().is_none());
    assert_eq!(sources(&s, "mo").await, vec!["m_during".to_string()]);
    s.done().await;
}

/// A save never waits on the unified store: here its table is locked by
/// another transaction for the whole save.
#[tokio::test(flavor = "multi_thread")]
async fn a_save_does_not_wait_on_the_unified_store() {
    let s = boot(V2_FOR_MO).await;
    let mut locker = s.store().await;
    let tx = locker.transaction().await.unwrap();
    tx.batch_execute("LOCK TABLE public.memory_sources IN ACCESS EXCLUSIVE MODE").await.unwrap();

    let save = s.k.emit("person:mo", memory("m_locked", "mo", "Saved while the store was locked"));
    tokio::time::timeout(std::time::Duration::from_secs(3), save).await.expect("the save waited on the unified store").unwrap();
    assert!(!s.k.memory_sync.is_synced("mo"));

    tx.rollback().await.unwrap();
    s.k.sync_memory().await.unwrap();
    assert_eq!(sources(&s, "mo").await, vec!["m_locked".to_string()]);
    s.done().await;
}

/// A trashed memory leaves the store at once, and comes back when restored.
#[tokio::test(flavor = "multi_thread")]
async fn a_trashed_memory_leaves_the_store_and_a_restored_one_returns() {
    let s = boot(V2_FOR_MO).await;
    s.k.emit("person:mo", memory("m_trash", "mo", "The gate code is 4512")).await.unwrap();
    s.k.sync_memory().await.unwrap();
    assert_eq!(sources(&s, "mo").await, vec!["m_trash".to_string()]);

    assert_eq!(s.k.trash("mo", false, "m_trash").await.unwrap(), Ok(()));
    assert!(!s.k.memory_sync.is_synced("mo"), "a trash takes the person out of step");
    s.k.sync_memory().await.unwrap();
    assert!(sources(&s, "mo").await.is_empty(), "the trashed memory's copy stayed in the store");

    assert!(s.k.restore("mo", "m_trash").await.unwrap());
    s.k.sync_memory().await.unwrap();
    assert_eq!(sources(&s, "mo").await, vec!["m_trash".to_string()], "a restored memory was not copied back");
    s.done().await;
}

/// One person whose copy keeps failing does not hold up anyone else.
#[tokio::test(flavor = "multi_thread")]
async fn one_failing_person_does_not_hold_up_the_rest() {
    let s = boot("[memory]\nembedding_dim = 4\n[memory.person_modes]\nmo = \"v2\"\nana = \"v2\"\n").await;
    let db = s.store().await;
    db.batch_execute(
        "CREATE FUNCTION refuse_ana() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
             IF NEW.owner_id = 'ana' THEN RAISE EXCEPTION 'ana refused'; END IF; RETURN NEW; END $$;
         CREATE TRIGGER refuse_ana BEFORE INSERT ON public.memory_sources FOR EACH ROW EXECUTE FUNCTION refuse_ana();",
    )
    .await
    .unwrap();
    s.k.emit("person:mo", memory("m_mo", "mo", "Mo's fact")).await.unwrap();
    s.k.emit("person:ana", memory("m_ana", "ana", "Ana's fact")).await.unwrap();

    assert!(s.k.sync_memory().await.is_err());
    assert!(s.k.memory_sync.is_synced("mo"), "mo was held up by ana's failure");
    assert_eq!(sources(&s, "mo").await, vec!["m_mo".to_string()]);
    assert!(!s.k.memory_sync.is_synced("ana"));

    db.batch_execute("DROP TRIGGER refuse_ana ON public.memory_sources").await.unwrap();
    s.k.sync_memory().await.unwrap();
    assert!(s.k.memory_sync.is_synced("ana"));
    s.done().await;
}

fn event(ev: Ev) -> Event {
    Event { seq: 1, at: chrono::Utc::now(), actor: "person:mo".into(), ev }
}

fn no_owner(_: &str) -> Option<String> {
    None
}

/// Bring `sync` to "mo is in step": a pass that took the start marks and settled mo.
fn in_step(sync: &MemorySync) {
    sync.begin_for_test();
    sync.settle_for_test("mo");
    sync.finish_for_test();
}

#[test]
fn nobody_is_in_step_until_a_pass_and_a_write_takes_them_out() {
    let sync = MemorySync::new();
    assert!(!sync.is_synced("mo"), "everyone is behind at start");
    in_step(&sync);
    assert!(sync.is_synced("mo"));

    sync.follow(&[event(memory("m1", "mo", "a fact"))], no_owner);
    assert!(!sync.is_synced("mo"), "a save takes the person out of step");
    in_step(&sync);
    assert!(sync.is_synced("mo"));

    sync.follow(&[event(Ev::Trashed { item: "m1".into(), person: "mo".into() })], no_owner);
    assert!(!sync.is_synced("mo"), "a trash takes the person out of step");
    in_step(&sync);
    sync.follow(&[event(Ev::Restored { item: "m1".into() })], |_| Some("mo".into()));
    assert!(!sync.is_synced("mo"), "a restore takes the memory's owner out of step");
}

/// A forget keeps everyone out of step until the pass that deletes it has
/// finished, not merely started.
#[test]
fn a_forget_holds_everyone_back_until_its_pass_has_finished() {
    let sync = MemorySync::new();
    in_step(&sync);
    sync.follow(&[event(Ev::MemoryForgotten { id: "m1".into() })], no_owner);
    assert!(!sync.is_synced("mo"));
    sync.begin_for_test();
    assert!(!sync.is_synced("mo"), "a forget being deleted by a running pass was served around");
    sync.finish_for_test();
    assert!(sync.is_synced("mo"));
}

#[test]
fn a_write_during_a_pass_keeps_the_person_out_of_step() {
    let sync = MemorySync::new();
    sync.begin_for_test();
    // The pass is working on mo when another save for mo lands.
    sync.follow(&[event(memory("m2", "mo", "another fact"))], no_owner);
    sync.settle_for_test("mo");
    sync.finish_for_test();
    assert!(!sync.is_synced("mo"), "the pass finished before the new save was copied");
}
