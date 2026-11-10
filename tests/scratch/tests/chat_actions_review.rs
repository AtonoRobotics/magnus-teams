//! Regressions from the review of master (2026-10-29), chat actions:
//! - sharing checks every turn the message draws on, not only its own;
//! - a shared message reaches its recipient only, not the owner too;
//! - a retry stops the superseded turn's task, and what it later posts
//!   stays hidden;
//! - "make a task" keeps the message's files;
//! - reading aloud a branch's copy reuses the answer's script.

use magnus_invariants::model::Script;
use magnus_invariants::{mcp, *};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

const CHAT: &str = "talking with the person you work for in a chat";
const LISTEN: &str = "Rewrite this answer to be heard";
static RELEASE: AtomicBool = AtomicBool::new(false);

fn said(c: &Conversation) -> String {
    let first = c.messages().first().cloned().unwrap_or_default();
    first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string()
}

fn model() -> Script {
    script(|c| async move {
        let system = c.body["system"].to_string();
        if system.contains(LISTEN) {
            return Reply::Text("Here is the answer, read aloud.".into());
        }
        if system.contains(CHAT) {
            let s = said(&c);
            return match c.turn() {
                0 if s.contains("CRM") => tool("crm__get_record", json!({ "id": "42" })),
                0 if s.contains("Book") => tool("create_task", json!({ "title": "Book the room", "goal": "Book the meeting room for Friday." })),
                _ => Reply::Text(format!("Answer to: {s}")),
            };
        }
        // Tasks: the booking waits until the test lets it go.
        if c.text().contains("Book the meeting room") {
            return match c.turn() {
                0 => contract_unchecked("book", "booked"),
                _ => {
                    while !RELEASE.load(Ordering::SeqCst) {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                    complete("booked")
                }
            };
        }
        match c.turn() {
            0 => contract_unchecked("do it", "done"),
            _ => complete("done"),
        }
    })
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

async fn entries(api: &Api, thread: &str) -> Vec<Value> {
    let (st, v) = api.get(&format!("/v1/chat/{thread}")).await;
    assert!(st.is_success(), "{v}");
    v["entries"].as_array().unwrap().clone()
}

async fn act(api: &Api, thread: &str, entry: &str, what: &str, body: Value) -> (reqwest::StatusCode, Value) {
    api.post(&format!("/v1/chat/{thread}/entries/{entry}/{what}"), body).await
}

async fn start() -> (Appliance, FakeModel) {
    let crm = mcp::crm().start().await;
    let m = FakeModel::start(model()).await;
    let app = Appliance::start(Spec::default().app("crm", &crm, "internal", "private", &[("get_record", "read")]).access("crm", &["finance"]), &m).await;
    Box::leak(Box::new(crm));
    (app, m)
}

#[tokio::test(flavor = "multi_thread")]
async fn sharing_checks_every_turn_it_draws_on_and_reaches_only_its_recipient() {
    let (app, _m) = start().await;
    let (alice, bob, carol) = (app.api("alice"), app.api("bob"), app.api("carol"));
    // Bob (finance) reads the CRM, then asks for a summary that uses no tool.
    let (thread, _) = say(&bob, "Look up the CRM record", None).await;
    let (_, summary) = say(&bob, "Summarise that in one line", Some(&thread)).await;
    let (st, v) = act(&bob, &thread, &format!("a:{summary}"), "share", json!({ "person": "carol" })).await;
    assert_eq!(st.as_u16(), 403, "CRM content reached someone the CRM refuses: {v}");

    // A clean answer is shared with carol, and with no one else.
    let (clean, hello) = say(&bob, "Say hello", None).await;
    let (st, v) = act(&bob, &clean, &format!("a:{hello}"), "share", json!({ "person": "carol" })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    assert!(carol.inbox().await.iter().any(|i| i["kind"] == "shared"));
    assert!(!alice.inbox().await.iter().any(|i| i["kind"] == "shared"), "the owner received a message shared with carol");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_retry_stops_the_superseded_task_and_keeps_its_later_result_hidden() {
    let (app, m) = start().await;
    let alice = app.api("alice");
    let (thread, turn) = say(&alice, "Book the room", None).await;
    let task = eventually("the booking task", PATIENCE, || async {
        alice.tasks().await["tasks"].as_array().unwrap().iter().find(|t| t["title"] == "Book the room").map(|t| t["task_id"].as_str().unwrap().to_string())
    })
    .await;
    let (st, v) = act(&alice, &thread, &format!("a:{turn}"), "resend", json!({})).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    RELEASE.store(true, Ordering::SeqCst);
    let old = alice.wait_settled(&task).await;
    assert_eq!(old["status"], "cancelled", "the superseded turn's task kept running: {old}");
    let new_turn = v["task_id"].as_str().unwrap().to_string();
    alice.wait_settled(&new_turn).await;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let shown = entries(&alice, &thread).await;
    assert!(!shown.iter().any(|e| e["id"] == format!("r:{task}")), "the superseded task's result came back: {shown:?}");
    let _ = m;
}

#[tokio::test(flavor = "multi_thread")]
async fn make_a_task_keeps_the_files_and_a_branch_reuses_the_spoken_script() {
    let (app, m) = start().await;
    let alice = app.api("alice");
    use base64::Engine;
    let (st, f) =
        alice.post("/v1/files", json!({ "name": "notes.txt", "media_type": "text/plain", "data_b64": base64::engine::general_purpose::STANDARD.encode("the notes") })).await;
    assert_eq!(st.as_u16(), 201, "{f}");
    let (st, v) = alice.post("/v1/chat", json!({ "text": "Summarise this", "files": [f["id"]] })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let (thread, turn) = (v["thread"].as_str().unwrap().to_string(), v["task_id"].as_str().unwrap().to_string());
    alice.wait_settled(&turn).await;
    let (st, t) = act(&alice, &thread, &format!("m:{turn}"), "task", json!({})).await;
    assert_eq!(st.as_u16(), 201, "{t}");
    let made = alice.task(t["task_id"].as_str().unwrap()).await;
    assert_eq!(made["files"].as_array().map(Vec::len), Some(1), "the task lost the message's file: {made}");

    // Read aloud once; the branch's copy reads the same script.
    let (st, s) = act(&alice, &thread, &format!("a:{turn}"), "spoken", json!({})).await;
    assert_eq!(st.as_u16(), 200, "{s}");
    let (_, b) = act(&alice, &thread, &format!("a:{turn}"), "branch", json!({})).await;
    let branch = b["thread"].as_str().unwrap().to_string();
    let listened = || m.requests().iter().filter(|c| c.body["system"].to_string().contains(LISTEN)).count();
    let before = listened();
    for _ in 0..2 {
        let (st, s2) = act(&alice, &branch, &format!("a:{turn}"), "spoken", json!({})).await;
        assert_eq!((st.as_u16(), s2["script"].clone()), (200, s["script"].clone()));
    }
    assert_eq!(listened(), before, "reading a branch's copy aloud called the model again");
}

#[tokio::test(flavor = "multi_thread")]
async fn read_aloud_speaks_as_the_character_and_logs_it() {
    let (app, m) = start().await;
    let alice = app.api("alice");
    let (thread, turn) = say(&alice, "Say hello", None).await;
    let (st, s) = act(&alice, &thread, &format!("a:{turn}"), "spoken", json!({})).await;
    assert_eq!(st.as_u16(), 200, "{s}");
    let heard = m.requests().into_iter().find(|c| c.body["system"].to_string().contains(LISTEN)).expect("no read-aloud call");
    assert!(heard.body["system"].to_string().contains("chief of staff"), "read aloud without the character: {}", heard.body["system"]);
    let db = app.db.admin().await;
    let rows: Vec<Value> = db.query("SELECT body FROM public.events WHERE kind = 'spoken_written'", &[]).await.unwrap().iter().map(|r| r.get::<_, Value>(0)).collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(rows[0]["character"].as_str().is_some_and(|c| c.starts_with("character.md@")), "no character logged: {rows:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn every_model_reply_logs_the_character_separate_from_the_persons_context() {
    let (app, _m) = start().await;
    let (bob, carol) = (app.api("bob"), app.api("carol"));
    // Carol's note enters her context, so her sent instructions differ from
    // Bob's; the character both speak with does not.
    let (st, v) = carol.post("/v1/relationship", json!({ "action": "add", "text": "Keep it brief (character-note)." })).await;
    assert_eq!(st.as_u16(), 200, "{v}");
    say(&carol, "What is on today?", None).await;
    say(&bob, "What is on my list?", None).await;
    let task = carol.submit("File the vendor summary.").await;
    carol.wait_settled(&task).await;
    let db = app.db.admin().await;
    let logged: Vec<Value> = db.query("SELECT body FROM public.events WHERE kind = 'model_call'", &[]).await.unwrap().iter().map(|r| r.get::<_, Value>(0)).collect();
    assert!(!logged.is_empty());
    let characters: std::collections::BTreeSet<String> = logged.iter().map(|b| b["context"]["character"].as_str().unwrap_or_default().to_string()).collect();
    assert_eq!(characters.len(), 1, "the character varies or is missing: {characters:?}");
    let only = characters.into_iter().next().unwrap();
    assert!(only.starts_with("character.md@"), "{only}");
    let instructions: std::collections::BTreeSet<String> = logged.iter().map(|b| b["context"]["instructions"].as_str().unwrap_or_default().to_string()).collect();
    assert!(instructions.len() > 1, "the sent instructions do not vary: {instructions:?}");
}
