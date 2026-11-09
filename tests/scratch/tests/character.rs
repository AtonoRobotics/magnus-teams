//! Identity and character (DECISIONS.md, "Identity and characters"): every
//! model call where Magnus speaks, chat or task, carries its identity and its
//! character, and the instructions version logged with each call is the hash
//! of exactly what that call was sent, character included.

use magnus_invariants::*;
use serde_json::{json, Value};
use sha2::Digest;

const IDENTITY: &str = "You speak and act as Magnus, never as the person.";
const CHARACTER: &str = "# Your character: chief of staff";
const BIBLE: &str = "You never mock the person.";

fn system(c: &Conversation) -> String {
    c.body["system"].as_str().map(String::from).unwrap_or_else(|| c.body["system"].to_string())
}

/// The hash the log gives a prompt (`name@hash`).
fn hash(s: &str) -> String {
    format!("{:x}", sha2::Sha256::digest(s.as_bytes()))[..12].to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn magnus_speaks_as_itself_in_its_character_on_every_call_and_the_log_says_which() {
    let s = script(|c| async move {
        if system(&c).contains("in a chat") {
            return Reply::Text("Handled.".into());
        }
        fail("nothing to do")
    });
    let model = FakeModel::start(s).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");

    let (st, v) = alice.post("/v1/chat", json!({ "text": "Hi" })).await;
    assert_eq!(st, 201, "{v}");
    alice.wait_status(v["task_id"].as_str().unwrap(), &["completed"]).await;
    let task = alice.submit("Look something up.").await;
    alice.wait_status(&task, &["failed"]).await;

    // The calls where Magnus speaks (not the kernel's own passes, such as
    // filing memory): a chat turn and a task.
    let all = model.requests();
    let speaking: Vec<String> = all.iter().map(system).filter(|s| s.starts_with("You are Magnus")).collect();
    assert!(speaking.iter().any(|s| s.contains("in a chat")) && speaking.iter().any(|s| !s.contains("in a chat")), "a chat turn and a task ran");
    for s in &speaking {
        assert!(s.contains(IDENTITY) && s.contains(CHARACTER) && s.contains(BIBLE), "a call without the identity or the character: {s}");
    }

    // Each logged call names instructions whose hash is a prompt Magnus
    // spoke with: the log says which character it spoke in.
    let sent: Vec<String> = speaking.iter().map(|s| hash(s)).collect();
    let db = app.db.admin().await;
    let logged: Vec<Value> = db.query("SELECT body FROM public.events WHERE kind = 'model_call'", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    let named: Vec<&str> = logged.iter().filter_map(|b| b["context"]["instructions"].as_str()).collect();
    assert!(!named.is_empty() && named.len() == logged.len(), "{logged:?}");
    for n in &named {
        let (_, h) = n.split_once('@').unwrap();
        assert!(sent.iter().any(|s| s == h), "{n} is not the hash of a prompt Magnus was sent ({sent:?})");
    }
}
