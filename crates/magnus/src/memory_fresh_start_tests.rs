//! The fresh start: a memory filed before the memory update is archived at
//! the boot after it, kept in the log and never served; one filed after is
//! served as before; a later boot archives nothing more; and erasing the
//! person still deletes the archived memories' content.

use super::FRESH_START_REASON;
use crate::admin_app::tests::{server_with, Server};
use crate::events::Ev;
use crate::tools::Label;
use axum::Router;
use serde_json::{json, Value};

/// Ages mo's `m_old*` memories to before the memory update.
const AGED: &str = "UPDATE public.events SET at = '2026-11-01T00:00:00Z' WHERE kind = 'memory' AND body->>'id' LIKE 'm_old%'";

fn memory(id: &str, fact: &str) -> Ev {
    Ev::Memory(crate::events::MemoryPayload {
        id: id.into(),
        person: "mo".into(),
        run: String::new(),
        fact: fact.into(),
        topic: None,
        label: Label::default(),
        sources: vec![],
        by: None,
    })
}

/// A server whose log holds two memories of mo's filed before the update,
/// one of them in the trash, and that server restarted on it.
async fn restarted_after_the_update() -> (Server, Server) {
    let s = server_with("", |_| Router::new()).await;
    s.k.emit("person:mo", memory("m_old", "Mo is allergic to peanuts")).await.unwrap();
    s.k.emit("person:mo", memory("m_old_trashed", "Mo keeps peanuts in the van")).await.unwrap();
    assert_eq!(s.k.trash("mo", false, "m_old_trashed").await.unwrap(), Ok(()));
    let r = s.restarted(AGED).await;
    (s, r)
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|m| m["id"].as_str().or(m["item"].as_str()).map(String::from)).collect()
}

/// What `memory_search` serves mo for `query`, by memory id.
async fn served(s: &Server, query: &str) -> Vec<String> {
    let run = crate::tools_core::tests::idle_run(s).await;
    s.k.memory_hits(&run, query).await.unwrap().into_iter().map(|m| m.entry.id).collect()
}

async fn count(s: &Server, sql: &str) -> i64 {
    s.store().await.query_one(sql, &[]).await.unwrap().get(0)
}

/// Not listed, summarised, searched, offered to the memory pass, taken as
/// a duplicate, forgotten, trashed or put back from the trash: the old
/// memory is history. Its event and content are still in the log.
#[tokio::test(flavor = "multi_thread")]
async fn an_old_memory_is_not_served_after_the_boot() {
    let (s, r) = restarted_after_the_update().await;
    let mo = r.token("mo").await;
    let (_, list) = r.get(&mo, "/v1/memory").await;
    assert!(ids(&list["memory"]).is_empty(), "{list}");
    let (_, summary) = r.get(&mo, "/v1/memory/summary").await;
    assert!(ids(&summary["clean_facts"]).is_empty() && ids(&summary["untrusted_facts"]).is_empty(), "{summary}");
    assert!(summary.get("archived_facts").is_none(), "archived facts are not served either: {summary}");
    assert!(served(&r, "peanuts").await.is_empty());
    assert!(served(&r, "").await.is_empty(), "nor listed as the newest");
    assert!(r.k.remembered("mo", |_| true).await.unwrap().is_empty(), "the memory pass's \"Already remembered\" and the duplicate check read this");
    let (status, _) = r.call(&mo, reqwest::Method::DELETE, "/v1/memory/m_old", None).await;
    assert_eq!(status, 404, "not there to forget");
    assert!(r.k.trash("mo", false, "m_old").await.unwrap().is_err(), "nor to trash");
    let (_, trash) = r.get(&mo, "/v1/trash").await;
    assert!(ids(&trash["items"]).is_empty(), "an old memory in the trash is not listed: {trash}");
    assert!(!r.k.restore("mo", "m_old_trashed").await.unwrap(), "nor put back");
    assert_eq!(count(&r, "SELECT count(*) FROM public.events WHERE kind = 'memory'").await, 2, "the log keeps them as history");
    assert_eq!(count(&r, "SELECT count(*) FROM public.content c JOIN public.events e USING (seq) WHERE e.kind = 'memory'").await, 2);
    s.done().await;
    r.done().await;
}

/// A fact remembered after the update is written (not "already
/// remembered", though an old memory says it), listed and searched.
#[tokio::test(flavor = "multi_thread")]
async fn a_new_memory_is_served() {
    let (s, r) = restarted_after_the_update().await;
    let run = crate::tools_core::tests::idle_run(&r).await;
    let (text, is_error) = crate::tools_core::tests::act(&r, &run, 1, "memory_write", json!({ "fact": "Mo is allergic to peanuts" })).await;
    assert!(!is_error && text.starts_with("remembered ("), "{text}");
    let new = r.k.read(|st| st.library.memory().iter().find(|m| m.run == run).map(|m| m.id.clone())).unwrap();
    let (_, list) = r.get(&r.token("mo").await, "/v1/memory").await;
    assert_eq!(ids(&list["memory"]), vec![new.clone()]);
    assert_eq!(served(&r, "peanuts").await, vec![new]);
    s.done().await;
    r.done().await;
}

/// The archive is written once: each old memory once, with its reason, and
/// a second boot writes no archive at all.
#[tokio::test(flavor = "multi_thread")]
async fn a_second_boot_archives_nothing() {
    let (s, r) = restarted_after_the_update().await;
    let archived = "SELECT count(*) FROM public.events WHERE kind = 'memory_archived'";
    assert_eq!(count(&r, archived).await, 2);
    assert_eq!(count(&r, &format!("SELECT count(*) FROM public.events WHERE kind = 'memory_archived' AND body->>'reason' = '{FRESH_START_REASON}'")).await, 2);
    let again = r.restarted("").await;
    assert_eq!(count(&again, archived).await, 2, "no new archive");
    let after_boot = "SELECT count(*) FROM public.events WHERE seq > (SELECT max(seq) FROM public.events WHERE kind = 'kernel_boot') AND kind LIKE 'memory%'";
    assert_eq!(count(&again, after_boot).await, 0);
    s.done().await;
    r.done().await;
    again.done().await;
}

/// Erasing the person deletes the content of their archived memories too,
/// in the library and in the trash.
#[tokio::test(flavor = "multi_thread")]
async fn erasing_the_person_removes_archived_memories_content() {
    let (s, r) = restarted_after_the_update().await;
    let seqs: Vec<i64> =
        r.k.read(|st| st.library.memory().iter().chain(st.trash.items().values().filter_map(|t| t.memory())).filter(|m| m.person == "mo").map(|m| m.seq).collect());
    assert_eq!(seqs.len(), 2);
    assert!(r.k.read(|st| st.memory_retired("m_old") && st.memory_retired("m_old_trashed")));
    r.k.erase_person("person:sam", "mo").await.unwrap();
    let left: i64 = r.store().await.query_one("SELECT count(*) FROM public.content WHERE seq = ANY($1)", &[&seqs]).await.unwrap().get(0);
    assert_eq!(left, 0, "an archived memory's content goes with its person");
    s.done().await;
    r.done().await;
}
