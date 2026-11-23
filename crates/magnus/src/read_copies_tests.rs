//! Removing an item takes its text out of every run that read it, against a
//! kernel booted on a fresh database with the content store walled by
//! person, as `magnus migrate` leaves it.
use crate::admin_app::tests::{server_with, Server};
use crate::events::Ev;
use crate::tools_core::tests::{act, idle_run};
use axum::Router;
use serde_json::json;

async fn end(s: &Server, run: &str) {
    s.k.emit("person:mo", Ev::Cancelled { run: run.into(), source: Some("console".into()), turn: None }).await.unwrap();
}

/// The content rows that hold `text`.
async fn holding(s: &Server, text: &str) -> i64 {
    s.store().await.query_one("SELECT count(*) FROM public.content WHERE strpos(body::text, $1) > 0", &[&text]).await.unwrap().get(0)
}

/// Run results in the state that hold `text`.
fn in_state(s: &Server, text: &str) -> usize {
    s.k.read(|st| st.runs.values().flat_map(|r| r.results.values()).filter(|x| x.text.contains(text)).count())
}

/// A memory searched in two later runs: deleted, its fact is in no
/// content row, neither where it was written nor where it was read.
#[tokio::test(flavor = "multi_thread")]
async fn removing_a_memory_takes_its_fact_out_of_the_runs_that_read_it() {
    let s = server_with("", |_| Router::new()).await;
    s.store().await.batch_execute(&crate::log::content_walls_sql("postgres")).await.unwrap();
    let fact = "my home address is 12 Elm Street";
    let writer = idle_run(&s).await;
    let (said, failed) = act(&s, &writer, 0, "memory_write", json!({ "fact": fact })).await;
    assert!(!failed, "{said}");
    end(&s, &writer).await;
    let id = s.k.read(|st| st.library.memory().iter().find(|m| m.run == writer).map(|m| m.id.clone())).unwrap();
    for n in 0..2 {
        let reader = idle_run(&s).await;
        let (found, _) = act(&s, &reader, n, "memory_search", json!({ "query": "home address" })).await;
        assert!(found.contains(fact), "search {n} did not find the fact: {found}");
        end(&s, &reader).await;
    }
    assert!(holding(&s, fact).await >= 3, "the fact was not where it was written and read");

    assert_eq!(s.k.trash("mo", false, &id).await.unwrap(), Ok(()));
    s.k.remove("person:mo", &id).await.unwrap();
    assert_eq!(holding(&s, fact).await, 0, "a content row still holds the deleted memory's fact");
    assert_eq!(in_state(&s, fact), 0, "a run's result in the state still holds the fact");
    s.done().await;
}

/// A knowledge entry read by a search: removed, its text is in no content row.
#[tokio::test(flavor = "multi_thread")]
async fn removing_a_knowledge_entry_takes_its_text_out_of_the_runs_that_read_it() {
    let s = server_with("", |_| Router::new()).await;
    s.store().await.batch_execute(&crate::log::content_walls_sql("postgres")).await.unwrap();
    crate::knowledge_index::tests::with_index(&s).await;
    let text = "the vault code is 4417";
    s.k.emit("person:mo", crate::knowledge_index::tests::entry("kb_vault", text, json!([["person:mo"]]))).await.unwrap();
    let reader = idle_run(&s).await;
    let (found, _) = act(&s, &reader, 0, "knowledge_search", json!({ "query": "vault code" })).await;
    assert!(found.contains(text), "the search did not find the entry: {found}");
    end(&s, &reader).await;

    assert_eq!(s.k.trash("mo", false, "kb_vault").await.unwrap(), Ok(()));
    s.k.remove("person:mo", "kb_vault").await.unwrap();
    assert_eq!(holding(&s, text).await, 0, "a content row still holds the removed entry's text");
    assert_eq!(in_state(&s, text), 0);
    s.done().await;
}
