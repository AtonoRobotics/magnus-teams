//! A person's data is theirs (the owner, 2026-11-11, replacing ruling 14):
//! a Magnus admin supports accounts and the server and reads no one's
//! tasks, conversations, files or memory, and nobody can make it otherwise.
//! The owner still finds someone's task to stop it, without its words.

use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn no_one_reads_a_persons_data_and_no_switch_opens_it() {
    let m = FakeModel::start(steps(vec![fail("nothing to do")])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));
    let task = carol.submit("Draft the quarterly letter.").await;
    carol.wait_settled(&task).await;
    assert_eq!(carol.get("/v1/data-access").await.1["private"], json!(true), "people's data is not private");

    for what in ["tasks", "chats", "files", "memory"] {
        let (st, v) = alice.get(&format!("/v1/people/carol/data/{what}")).await;
        assert_eq!(st.as_u16(), 403, "the owner read carol's {what}: {v}");
    }
    assert_eq!(carol.get("/v1/people/alice/data/tasks").await.0.as_u16(), 403);
    assert_eq!(carol.put("/v1/data-access", json!({ "private": false })).await.0.as_u16(), 403);
    assert_eq!(alice.put("/v1/data-access", json!({ "private": false })).await.0.as_u16(), 409, "the owner opened people's data");
    assert_eq!(alice.get("/v1/people/carol/data/tasks").await.0.as_u16(), 403);
    assert_eq!(alice.get("/v1/people/alice/data/tasks").await.0.as_u16(), 200, "the owner cannot read their own");
    let reads =
        app.db.admin().await.query_one("SELECT count(*) FROM public.events WHERE kind = 'person_data_read' AND body->>'person' = 'carol'", &[]).await.unwrap().get::<_, i64>(0);
    assert_eq!(reads, 0);
}

/// The owner's list of everyone's tasks in conversation names other
/// people's tasks, so the owner can stop one for the server's safety, and
/// carries none of their words.
#[tokio::test(flavor = "multi_thread")]
async fn the_owners_list_of_everyones_tasks_in_conversation_carries_no_ones_words() {
    const SAID: &str = "Bob's raise is eight percent.";
    let m = FakeModel::start(script(|c| async move {
        if !c.body["system"].to_string().contains("talking with the person you work for in a chat") {
            return fail(SAID);
        }
        match c.turn() {
            0 => tool("list_tasks", json!({})),
            _ => Reply::Text(c.last_results().first().map(|r| r.text.clone()).unwrap_or_default()),
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let task = bob.submit("Note my raise.").await;
    bob.wait_settled(&task).await;
    let (st, v) = alice.post("/v1/chat", json!({ "text": "What is everyone working on?" })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let (thread, turn) = (v["thread"].as_str().unwrap().to_string(), v["task_id"].as_str().unwrap().to_string());
    let said = eventually("magnus answers with the list", PATIENCE, || async {
        let (_, t) = alice.get(&format!("/v1/chat/{thread}")).await;
        t["entries"].as_array().into_iter().flatten().find(|e| e["kind"] == "answer" && e["task_id"] == turn.as_str()).map(|a| a["text"].as_str().unwrap_or_default().to_string())
    })
    .await;
    assert!(said.contains(&task), "the owner can no longer find bob's task to stop it: {said}");
    assert!(!said.contains(SAID), "the owner read bob's task: {said}");
    let reads =
        app.db.admin().await.query_one("SELECT count(*) FROM public.events WHERE kind = 'person_data_read' AND body->>'person' = 'bob'", &[]).await.unwrap().get::<_, i64>(0);
    assert_eq!(reads, 0);
}
