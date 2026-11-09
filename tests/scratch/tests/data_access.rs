//! The admin's access to people's data is a policy (owner's ruling 14,
//! 2026-11-01): by default a person's tasks, conversations, files and memory
//! are the organisation's, and the owner reads them; each read is in the log
//! and in the person's inbox. The owner can make people's data private.

use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn the_admin_reads_a_persons_data_unless_the_policy_makes_it_private() {
    let m = FakeModel::start(steps(vec![fail("nothing to do")])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));
    let task = carol.submit("Draft the quarterly letter.").await;
    carol.wait_settled(&task).await;
    assert_eq!(carol.get("/v1/data-access").await.1["private"], json!(false), "people's data is private by default");

    // The owner reads carol's tasks: hers, and the read is told to her.
    let (st, v) = alice.get("/v1/people/carol/data/tasks").await;
    assert_eq!(st.as_u16(), 200, "{v}");
    assert!(v["tasks"].as_array().unwrap().iter().any(|t| t["task_id"] == json!(task)), "{v}");
    for what in ["chats", "files", "memory"] {
        assert_eq!(alice.get(&format!("/v1/people/carol/data/{what}")).await.0.as_u16(), 200, "{what}");
    }
    let told = eventually("carol is told her tasks were read", PATIENCE, || async {
        carol.inbox().await.into_iter().find(|i| i["details"]["reason"] == "data_read" && i["details"]["evidence"]["what"] == "tasks")
    })
    .await;
    assert_eq!(told["details"]["evidence"]["by"], "alice", "{told}");
    let reads =
        app.db.admin().await.query_one("SELECT count(*) FROM public.events WHERE kind = 'person_data_read' AND body->>'person' = 'carol'", &[]).await.unwrap().get::<_, i64>(0);
    assert_eq!(reads, 4, "each read is in the log");

    // Only the owner reads others' data, and only the owner sets the policy.
    assert_eq!(carol.get("/v1/people/alice/data/tasks").await.0.as_u16(), 403);
    assert_eq!(carol.put("/v1/data-access", json!({ "private": true })).await.0.as_u16(), 403);

    // Private: the owner reads no one's data but their own.
    assert_eq!(alice.put("/v1/data-access", json!({ "private": true })).await.0.as_u16(), 204);
    let (st, v) = alice.get("/v1/people/carol/data/tasks").await;
    assert_eq!(st.as_u16(), 403, "the owner read carol's tasks while people's data is private: {v}");
    assert_eq!(alice.get("/v1/people/alice/data/tasks").await.0.as_u16(), 200);
    assert_eq!(alice.put("/v1/data-access", json!({ "private": false })).await.0.as_u16(), 204);
    assert_eq!(alice.get("/v1/people/carol/data/tasks").await.0.as_u16(), 200);
}

/// The owner's list of everyone's tasks in conversation is a read of their
/// data like the Data tab's: with people's data the organisation's it is in
/// the log and each person is told; with it private, other people's tasks
/// are listed (the owner still controls them) without their words.
#[tokio::test(flavor = "multi_thread")]
async fn the_owners_list_of_everyones_tasks_in_conversation_follows_the_policy_on_peoples_data() {
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
    let reads = || async {
        app.db.admin().await.query_one("SELECT count(*) FROM public.events WHERE kind = 'person_data_read' AND body->>'person' = 'bob'", &[]).await.unwrap().get::<_, i64>(0)
    };
    let listed = |n: usize| {
        let alice = &alice;
        async move {
            let (st, v) = alice.post("/v1/chat", json!({ "text": format!("What is everyone working on? ({n})") })).await;
            assert_eq!(st.as_u16(), 201, "{v}");
            let (thread, turn) = (v["thread"].as_str().unwrap().to_string(), v["task_id"].as_str().unwrap().to_string());
            eventually("magnus answers with the list", PATIENCE, || async {
                let (_, t) = alice.get(&format!("/v1/chat/{thread}")).await;
                t["entries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|e| e["kind"] == "answer" && e["task_id"] == turn.as_str())
                    .map(|a| a["text"].as_str().unwrap_or_default().to_string())
            })
            .await
        }
    };

    // The organisation's: Bob's task with its words, the read in the log and told to him.
    let said = listed(0).await;
    assert!(said.contains(&task) && said.contains(SAID), "{said}");
    assert_eq!(reads().await, 1, "the owner's list of bob's tasks is not in the log");
    let told = eventually("bob is told his tasks were read", PATIENCE, || async {
        bob.inbox().await.into_iter().find(|i| i["details"]["reason"] == "data_read" && i["details"]["evidence"]["what"] == "tasks")
    })
    .await;
    assert_eq!(told["details"]["evidence"]["by"], "alice", "{told}");

    // Private: Bob's task is listed, its words are not, and nothing was read.
    assert_eq!(alice.put("/v1/data-access", json!({ "private": true })).await.0.as_u16(), 204);
    let said = listed(1).await;
    assert!(said.contains(&task), "the owner can no longer find bob's task to control it: {said}");
    assert!(!said.contains(SAID), "the owner read bob's task while people's data is private: {said}");
    assert_eq!(reads().await, 1);
}
