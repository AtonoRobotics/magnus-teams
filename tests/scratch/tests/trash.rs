//! The trash (deletion plan, step 2). Every kind of item a person makes goes
//! to the trash out of every view, comes back when restored, and when
//! removed its content leaves the content store while the log keeps the
//! record. Only the item goes. A person reaches only their own trash; the
//! trash empties itself after `[trash] keep_days`.

use base64::Engine;
use magnus_invariants::model::Script;
use magnus_invariants::*;
use serde_json::json;

const CHAT: &str = "talking with the person you work for in a chat";

/// Chat answers "Answer to: <what was said>". A task asked to remember
/// writes the memory; every task then ends (failed, so it waits for no
/// review). The memory pass keeps nothing.
fn model() -> Script {
    script(|c| async move {
        let system = c.body["system"].to_string();
        let said = c.messages().first().map(|m| m.to_string()).unwrap_or_default();
        if system.contains(CHAT) {
            let first = c.messages().first().cloned().unwrap_or_default();
            let words = first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string();
            return Reply::Text(format!("Answer to: {words}"));
        }
        if !said.contains("Remember") || system.contains("long-term memory") {
            return match c.turn() {
                0 if !system.contains("long-term memory") => contract_unchecked("do the work", "the work is done"),
                _ => fail("done"),
            };
        }
        match c.turn() {
            0 => contract_unchecked("remember", "remembered"),
            1 => tool("memory_write", json!({ "fact": "Alice likes ferret reports", "topic": "reports" })),
            _ => fail("done"),
        }
    })
}

fn b64(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(s)
}

async fn say(api: &Api, text: &str, thread: Option<&str>) -> (String, String) {
    let mut body = json!({ "text": text });
    if let Some(t) = thread {
        body["thread"] = json!(t);
    }
    let (st, v) = api.post("/v1/chat", body).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let (thread, turn) = (v["thread"].as_str().unwrap().to_string(), v["task_id"].as_str().unwrap().to_string());
    api.wait_settled(&turn).await;
    (thread, turn)
}

/// Everything the person's views show, as one text.
async fn shown(api: &Api, threads: &[&str]) -> String {
    let mut all = String::new();
    for path in ["/v1/tasks", "/v1/chat", "/v1/files", "/v1/projects", "/v1/memory", "/v1/knowledge"] {
        all += &api.get(path).await.1.to_string();
    }
    for t in threads {
        all += &api.get(&format!("/v1/chat/{t}")).await.1.to_string();
    }
    all
}

async fn store(app: &Appliance) -> String {
    let rows = app.db.admin().await.query("SELECT body::text FROM public.content", &[]).await.unwrap();
    rows.iter().map(|r| r.get::<_, String>(0)).collect::<Vec<_>>().join("\n")
}

async fn trash(api: &Api, item: &str) -> u16 {
    api.post("/v1/trash", json!({ "item": item })).await.0.as_u16()
}

#[tokio::test(flavor = "multi_thread")]
async fn each_kind_of_item_goes_to_the_trash_comes_back_and_is_removed() {
    let m = FakeModel::start(model()).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));

    let task = alice.submit("Remember that I like ferret reports.").await;
    alice.wait_settled(&task).await;
    let memory = alice.get("/v1/memory").await.1["memory"][0]["id"].as_str().expect("the task wrote no memory").to_string();
    let (conversation, _) = say(&alice, "Tell me about bison", None).await;
    say(&alice, "And more bison", Some(&conversation)).await;
    let (kept, _) = say(&alice, "Hello there", None).await;
    let (_, turn) = say(&alice, "A cheetah question", Some(&kept)).await;
    let (st, f) = alice.post("/v1/files", json!({ "name": "dingo.txt", "media_type": "text/plain", "data_b64": b64("dingo notes") })).await;
    assert_eq!(st, 201, "{f}");
    let file = f["id"].as_str().unwrap().to_string();
    let (st, p) = alice.post("/v1/projects", json!({ "name": "emu project", "instructions": "Mind the emu." })).await;
    assert!(st.is_success(), "{p}");
    let project = p["id"].as_str().unwrap().to_string();
    let (st, k) = carol.post("/v1/knowledge", json!({ "title": "gecko facts", "text": "Geckos climb glass.", "groups": ["staff"] })).await;
    assert_eq!(st, 201, "{k}");
    let knowledge = k["id"].as_str().unwrap().to_string();

    let words = ["ferret", "bison", "cheetah", "dingo", "emu", "gecko"];
    let before = shown(&alice, &[&conversation, &kept]).await + &shown(&carol, &[]).await;
    for w in words {
        assert!(before.contains(w), "{w} is not shown before: {before}");
    }

    let mine = [task.clone(), memory.clone(), conversation.clone(), format!("m:{turn}"), format!("a:{turn}"), file.clone(), project.clone()];
    for item in &mine {
        assert_eq!(trash(&alice, item).await, 204, "{item}");
    }
    assert_eq!(trash(&alice, &knowledge).await, 204, "the owner may delete a knowledge entry");
    let after = shown(&alice, &[&conversation, &kept]).await + &shown(&carol, &[]).await;
    for w in words {
        assert!(!after.contains(w), "{w} is still shown from the trash: {after}");
    }
    assert!(after.contains("Hello there"), "the rest of the conversation went too: {after}");
    let listed = alice.get("/v1/trash").await.1;
    let kinds: Vec<&str> = listed["items"].as_array().unwrap().iter().filter_map(|i| i["kind"].as_str()).collect();
    for kind in ["task", "memory", "conversation", "message", "file", "project", "knowledge"] {
        assert!(kinds.contains(&kind), "no {kind} in the trash: {listed}");
    }

    // Restored, all of it is back where it was.
    for item in mine.iter().chain([&knowledge]) {
        assert_eq!(alice.post(&format!("/v1/trash/{item}/restore"), json!({})).await.0, 204, "{item}");
    }
    let back = shown(&alice, &[&conversation, &kept]).await + &shown(&carol, &[]).await;
    for w in words {
        assert!(back.contains(w), "{w} did not come back: {back}");
    }

    // Removed, the content leaves the store; the record stays.
    for item in mine.iter().chain([&knowledge]) {
        assert_eq!(trash(&alice, item).await, 204, "{item}");
    }
    let (st, v) = alice.post("/v1/trash/empty", json!({})).await;
    assert_eq!((st.as_u16(), v["removed"].clone()), (200, json!(mine.len() + 1)), "{v}");
    let left = store(&app).await;
    for w in words {
        assert!(!left.to_lowercase().contains(w), "{w} is still in the content store");
    }
    assert!(left.contains("Hello there"), "removing a message took the rest of its conversation");
    let events: i64 = app.db.admin().await.query_one("SELECT count(*) FROM public.events WHERE body->>'task' = $1", &[&task]).await.unwrap().get(0);
    assert!(events > 0, "the task's record went with its content");

    app.restart().await;
    let after = shown(&alice, &[&conversation, &kept]).await + &shown(&carol, &[]).await;
    for w in words {
        assert!(!after.contains(w), "{w} came back after a restart: {after}");
    }
    assert!(alice.get("/v1/trash").await.1["items"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_reaches_only_their_own_trash() {
    let m = FakeModel::start(model()).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (alice, bob, carol) = (app.api("alice"), app.api("bob"), app.api("carol"));
    let task = alice.submit("Plan the offsite.").await;
    alice.wait_settled(&task).await;
    let (_, k) = carol.post("/v1/knowledge", json!({ "title": "Parking", "text": "Park on level 2.", "groups": ["staff"] })).await;
    let knowledge = k["id"].as_str().unwrap().to_string();

    assert_eq!(trash(&bob, &task).await, 404, "bob deleted alice's task");
    assert_eq!(trash(&bob, &knowledge).await, 404, "bob deleted carol's knowledge entry");
    assert_eq!(trash(&carol, &knowledge).await, 204, "carol could not delete her own entry");
    assert_eq!(trash(&alice, &task).await, 204);
    assert!(bob.get("/v1/trash").await.1["items"].as_array().unwrap().is_empty(), "bob sees others' trash");
    assert_eq!(bob.post(&format!("/v1/trash/{task}/restore"), json!({})).await.0, 404, "bob restored alice's task");
    assert_eq!(bob.send(reqwest::Method::DELETE, &format!("/v1/trash/{task}"), None).await.0, 404, "bob removed alice's task");
    assert_eq!(bob.post("/v1/trash/empty", json!({})).await.1["removed"], 0);
    assert_eq!(alice.get("/v1/trash").await.1["items"].as_array().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_trash_empties_itself_after_its_keeping_time() {
    let m = FakeModel::start(model()).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let task = alice.submit("Plan the narwhal survey.").await;
    alice.wait_settled(&task).await;
    assert_eq!(trash(&alice, &task).await, 204);
    assert!(store(&app).await.contains("narwhal"));

    // Kept for no days: removed when the appliance next looks, at its start.
    app.kill().await;
    let config = app.dir.path().join("magnus.toml");
    let toml = std::fs::read_to_string(&config).unwrap() + "\n[trash]\nkeep_days = 0\n";
    std::fs::write(&config, toml).unwrap();
    app.restart().await;
    for _ in 0..50 {
        if alice.get("/v1/trash").await.1["items"].as_array().is_some_and(|i| i.is_empty()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(alice.get("/v1/trash").await.1["items"].as_array().unwrap().is_empty(), "the trash kept an item past its time");
    assert!(!store(&app).await.contains("narwhal"), "the item's content is still in the store");
}
