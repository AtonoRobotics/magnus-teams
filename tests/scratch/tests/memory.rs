//! Memory (M1, M3): what the person tells Magnus is kept and recalled in a
//! later conversation. The memory pass (a removable module, K23) reads each
//! ended exchange and files durable facts through `execute()`, with the
//! run's taint and its source; its ablation is the same exchange with the
//! module off, where nothing is kept. An explicit "remember that ..." is
//! written in the chat turn itself, which may write the person's memory and
//! nothing else, and a memory write is not counted as sending data out.

use magnus_invariants::model::Script;
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::{json, Value};

const CHAT: &str = "talking with the person you work for in a chat";
const PASS: &str = "You keep the person's long-term memory";

fn said(c: &Conversation) -> String {
    let first = c.messages().first().cloned().unwrap_or_default();
    first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string()
}

/// The person's model: the memory pass files the allergy it reads about;
/// chat answers from memory when asked, and writes when told to remember.
fn model() -> Script {
    script(|c| async move {
        let system = c.body["system"].to_string();
        if system.contains(PASS) {
            let text = c.text();
            let (_, known) = text.split_once("Already remembered").unwrap_or_default();
            return if text.contains("allergic to peanuts") && !known.contains("peanuts") {
                tool("memory_write", json!({ "fact": "The person is allergic to peanuts." }))
            } else {
                Reply::Text(String::new())
            };
        }
        assert!(system.contains(CHAT), "only chat in this test");
        let s = said(&c);
        match c.turn() {
            0 if s.contains("Remember that") => tool("memory_write", json!({ "fact": "The person's sister is called Ana." })),
            0 if s.contains("What should I avoid") => tool("memory_search", json!({ "query": "allergy food" })),
            // What the live model did when asked (2026-10-29): search with nothing.
            0 if s.contains("what do you have in memory") => tool("memory_search", json!({ "query": "" })),
            _ if s.contains("what do you have in memory") => Reply::Text(
                if c.text().contains("allergic to peanuts") { "I remember that you are allergic to peanuts." } else { "I don't have any saved memories about you yet." }.into(),
            ),
            0 => Reply::Text("Noted, thanks.".into()),
            _ if c.text().contains("allergic to peanuts") => Reply::Text("Avoid peanuts: you told me you are allergic.".into()),
            _ => Reply::Text("Done.".into()),
        }
    })
}

async fn chat(api: &Api, text: &str) -> String {
    let (st, v) = api.post("/v1/chat", json!({ "text": text })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let task = v["task_id"].as_str().unwrap().to_string();
    let done = api.wait_settled(&task).await;
    done["summary"].as_str().or(done["result"].as_str()).unwrap_or_default().to_string()
}

async fn memories(api: &Api) -> Vec<Value> {
    api.get("/v1/memory").await.1["memory"].as_array().cloned().unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fact_stated_in_one_chat_is_recalled_in_a_new_one() {
    let m = FakeModel::start(model()).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    chat(&alice, "By the way, I'm allergic to peanuts.").await;
    let kept = eventually("the memory pass files the fact", PATIENCE, || async {
        let m = memories(&alice).await;
        (!m.is_empty()).then_some(m)
    })
    .await;
    assert_eq!(kept.len(), 1, "{kept:?}");
    let f = &kept[0];
    assert_eq!((f["fact"].as_str(), f["by"].as_str()), (Some("The person is allergic to peanuts."), Some("memory_pass")), "{f}");
    assert!(f["at"].is_string() && f["run"].is_string() && f["label"].is_object(), "no time, source or provenance: {f}");

    // A new conversation recalls it.
    let answer = chat(&alice, "What should I avoid at the restaurant tonight?").await;
    assert!(answer.contains("Avoid peanuts"), "the new chat did not recall the fact: {answer}");
    // The pass does not file what is already kept.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert_eq!(memories(&alice).await.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn without_the_memory_pass_a_stated_fact_is_not_kept_but_remember_that_writes_at_once() {
    let m = FakeModel::start(model()).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    reconfigure(&mut app, |c| {
        c.insert("modules".into(), toml::Value::Table(toml::toml! { memory_pass = false }));
    })
    .await;
    let alice = app.api("alice");
    chat(&alice, "By the way, I'm allergic to peanuts.").await;
    let answer = chat(&alice, "What should I avoid at the restaurant tonight?").await;
    assert!(!answer.contains("peanuts"), "recalled without the memory pass: {answer}");
    assert!(memories(&alice).await.is_empty());
    assert!(!m.requests().iter().any(|c| c.body["system"].to_string().contains(PASS)), "the memory pass ran while switched off");

    // An explicit request is written in the turn itself; chat is offered the write.
    chat(&alice, "Remember that my sister is called Ana.").await;
    let kept = memories(&alice).await;
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!((kept[0]["fact"].as_str(), kept[0]["by"].as_str()), (Some("The person's sister is called Ana."), Some("model")), "{kept:?}");
    let turn = m.requests().into_iter().find(|c| said(c).contains("Remember that")).unwrap();
    assert!(turn.offered_tools().contains(&"memory_write".to_string()));
}

/// Bug (owner, 2026-10-29): asked "what do you have in memory?", the model
/// searched with an empty query, which matched nothing, and it said it had no
/// memories while two were kept. An empty query lists what is remembered.
#[tokio::test(flavor = "multi_thread")]
async fn asked_what_it_remembers_it_lists_what_is_kept() {
    let m = FakeModel::start(model()).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    chat(&alice, "By the way, I'm allergic to peanuts.").await;
    eventually("the memory pass files the fact", PATIENCE, || async { (!memories(&alice).await.is_empty()).then_some(()) }).await;
    let answer = chat(&alice, "what do you have in memory?").await;
    assert!(answer.contains("allergic to peanuts"), "the person's memory was not listed: {answer}");
}
