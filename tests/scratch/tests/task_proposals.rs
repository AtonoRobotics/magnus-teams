//! Task proposals are suggestions (spec rev 140, Permissions and tasks).
//! `propose_task` and `propose_schedule` are gone: what the person asks for
//! is started at once (`create_task`, `create_schedule`), and Magnus's own
//! ideas go through `suggest`. From a chat turn a suggestion is a card in
//! the thread; from a task or a review, which have no thread, it is a
//! `suggestion` in the person's inbox. Either way one accept starts it, on
//! the server's per-task budget, with the suggesting run's taint; nothing
//! starts before that, and only the person it was put to can accept it.

use magnus_invariants::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const CHAT: &str = "talking with the person you work for in a chat";
const PASS: &str = "You keep the person's long-term memory";
const TITLE: &str = "Chase the overdue invoice";
const GOAL: &str = "Draft a reminder to Acme about invoice 1042 (task-prop-31).";
const FIRST: &str = "Close CRM record 42.";
const SECOND: &str = "Look over my invoices.";
/// The harness's per-task `max_usd`.
const SERVER_LIMIT: f64 = 50.0;

fn said(c: &Conversation) -> String {
    let first = c.messages().first().cloned().unwrap_or_default();
    first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string()
}

/// The system context as text, whether sent as a string or as blocks.
fn system(c: &Conversation) -> String {
    match &c.body["system"] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        other => other.to_string(),
    }
}

/// The id of this turn's `list_tasks` call.
fn read_call(c: &Conversation) -> Value {
    let blocks = c.messages().into_iter().flat_map(|m| m["content"].as_array().cloned().unwrap_or_default());
    blocks.into_iter().find(|b| b["type"] == "tool_use" && b["name"] == "list_tasks").map(|b| b["id"].clone()).expect("no list_tasks call")
}

fn suggestion(evidence: Value, extra: Value) -> Value {
    let mut v = json!({ "title": TITLE, "goal": GOAL, "evidence": evidence, "permissions": ["Reads your invoices", "Drafts an email; sends nothing"] });
    for (k, x) in extra.as_object().cloned().unwrap_or_default() {
        v[k] = x;
    }
    v
}

async fn chat(api: &Api, text: &str) -> (String, String) {
    let (st, v) = api.post("/v1/chat", json!({ "text": text })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let task = v["task_id"].as_str().unwrap().to_string();
    api.wait_settled(&task).await;
    (v["thread"].as_str().unwrap_or_default().to_string(), task)
}

/// Wait until the thread has an entry matching `f`.
async fn wait_entry(api: &Api, thread: &str, f: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..600 {
        let (_, v) = api.get(&format!("/v1/chat/{thread}")).await;
        if let Some(e) = v["entries"].as_array().into_iter().flatten().find(|e| f(e)) {
            return e.clone();
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("no such entry in the thread {thread}");
}

/// In chat the proposal tools are not offered, and a call to one is
/// refused; a suggestion is a card the person accepts with one action,
/// which starts one task for them on the server's per-task budget.
#[tokio::test(flavor = "multi_thread")]
async fn in_chat_a_suggestion_is_accepted_with_one_action_and_the_proposal_tools_are_gone() {
    let m = FakeModel::start(script(|c| async move {
        let sys = system(&c);
        if sys.contains(PASS) {
            return Reply::Text(String::new());
        }
        if !sys.contains(CHAT) {
            return match c.turn() {
                0 => plan("do the suggested work"),
                _ => fail("done"),
            };
        }
        match c.turn() {
            0 => tool("propose_task", json!({ "title": TITLE, "text": GOAL })),
            1 => tool("list_tasks", json!({})),
            2 => tool("suggest", suggestion(json!([{ "call": read_call(&c), "shows": "The invoice task is still open." }]), json!({}))),
            _ => Reply::Text("The card awaits you.".into()),
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (thread, _) = chat(&alice, "Anything I should deal with?").await;
    let chat_turns: Vec<Conversation> = m.requests().into_iter().filter(|c| system(c).contains(CHAT)).collect();
    for t in ["propose_task", "propose_schedule"] {
        assert!(!chat_turns[0].offered_tools().iter().any(|n| n == t), "{t} is still offered in chat");
    }
    let refused = &chat_turns.iter().find(|c| c.turn() == 1).expect("turn 1").last_results()[0];
    assert!(refused.is_error, "propose_task was carried out: {}", refused.text);
    let card = wait_entry(&alice, &thread, |e| e["kind"] == "suggestion_card").await;
    assert_eq!((&card["state"], &card["title"], &card["plan"]), (&json!("open"), &json!(TITLE), &json!(GOAL)), "{card}");
    assert!(!thread.is_empty() && !wait_entry(&alice, &thread, |e| e["kind"] == "suggestion_card").await["card"].is_null());
    let id = card["card"].as_str().unwrap();
    assert_eq!(alice.tasks().await["tasks"].as_array().unwrap().len(), 0, "work started before it was accepted");
    let path = format!("/v1/chat/cards/{id}");
    assert_eq!(app.api("bob").post(&path, json!({ "decision": "accept" })).await.0.as_u16(), 404, "someone else accepted the card");
    // Accepting twice at once starts one task.
    let answers = futures::future::join_all((0..6).map(|_| alice.post(&path, json!({ "decision": "accept" })))).await;
    let ok: Vec<&Value> = answers.iter().filter(|(st, _)| st.is_success()).map(|(_, v)| v).collect();
    assert_eq!(ok.len(), 1, "{answers:?}");
    let task = ok[0]["task"].as_str().expect("no task started").to_string();
    let t = alice.wait_settled(&task).await;
    assert_eq!(t["person"], "alice", "{t}");
    assert_eq!(alice.task(&task).await["budget"]["max_usd"].as_f64(), Some(SERVER_LIMIT), "the task is not on the server's budget");
    assert_eq!(alice.tasks().await["tasks"].as_array().unwrap().len(), 1, "one accept started more than one task");
}

/// A task has no thread: its suggestion goes to the person's inbox, and one
/// accept starts the work with the suggesting run's taint. A suggestion of
/// a schedule sets the schedule up on that one accept.
#[tokio::test(flavor = "multi_thread")]
async fn a_task_suggests_work_to_the_inbox_and_one_accept_starts_it() {
    let first: Arc<Mutex<String>> = Arc::default();
    let f = first.clone();
    let m = FakeModel::start(script(move |c| {
        let f = f.clone();
        async move {
            let sys = system(&c);
            if sys.contains(PASS) {
                return Reply::Text(String::new());
            }
            let ev = || json!([{ "task": f.lock().unwrap().clone(), "shows": "Record 42 is closed; the invoice is not." }]);
            match (said(&c).as_str(), c.turn()) {
                (FIRST, 0) => plan("close it"),
                (FIRST, _) => fail("done"),
                (SECOND, 0) => plan("look over invoices"),
                (SECOND, 1) => tool("suggest", suggestion(ev(), json!({}))),
                (SECOND, 2) => tool("suggest", suggestion(ev(), json!({ "title": "Weekly invoice check", "goal": "Check the invoices (sched-sug-5).", "every_minutes": 10080 }))),
                (SECOND, _) => fail("done"),
                (GOAL, 0) => plan("draft it"),
                _ => fail("done"),
            }
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let t1 = alice.submit(FIRST).await;
    *first.lock().unwrap() = t1.clone();
    alice.wait_settled(&t1).await;
    let t2 = alice.submit(SECOND).await;
    alice.wait_settled(&t2).await;
    let offered = m.requests().into_iter().find(|c| said(c) == SECOND).expect("the task ran").offered_tools();
    assert!(offered.iter().any(|t| t == "suggest") && !offered.iter().any(|t| t == "propose_schedule" || t == "propose_task"), "{offered:?}");
    let items: Vec<Value> = alice.inbox().await.into_iter().filter(|i| i["kind"] == "suggestion" && i["status"] == "open").collect();
    assert_eq!(items.len(), 2, "{items:?}");
    let once = items.iter().find(|i| i["details"]["every_minutes"].is_null()).expect("the one-off suggestion");
    let weekly = items.iter().find(|i| i["details"]["every_minutes"] == 10080).expect("the schedule suggestion");
    assert_eq!(once["details"]["evidence"][0]["task"], t1.as_str(), "{once}");
    let before = alice.tasks().await["tasks"].as_array().unwrap().len();
    assert!(alice.get("/v1/schedules").await.1["schedules"].as_array().unwrap().is_empty(), "scheduled before it was accepted");

    // Only its person accepts it; one accept starts the task.
    let path = |i: &Value| format!("/v1/inbox/{}", i["id"].as_str().unwrap());
    assert!(!app.api("bob").post(&path(once), json!({ "decision": "accept" })).await.0.is_success(), "someone else accepted it");
    let (st, v) = alice.post(&path(once), json!({ "decision": "accept" })).await;
    assert!(st.is_success(), "{v}");
    let started = eventually("the accepted suggestion's task", Duration::from_secs(60), || {
        let alice = alice.clone();
        async move { alice.tasks().await["tasks"].as_array().unwrap().iter().find(|t| t["text"] == GOAL).cloned() }
    })
    .await;
    let t = alice.task(started["task_id"].as_str().unwrap()).await;
    assert_eq!((&t["person"], t["budget"]["max_usd"].as_f64()), (&json!("alice"), Some(SERVER_LIMIT)), "{t}");
    assert_eq!(alice.tasks().await["tasks"].as_array().unwrap().len(), before + 1);

    // The schedule is set up on its one accept.
    let (st, v) = alice.post(&path(weekly), json!({ "decision": "accept" })).await;
    assert!(st.is_success(), "{v}");
    let (_, s) = alice.get("/v1/schedules").await;
    let list = s["schedules"].as_array().unwrap();
    assert_eq!(list.len(), 1, "{s}");
    assert_eq!((&list[0]["title"], &list[0]["every_minutes"], &list[0]["person"]), (&json!("Weekly invoice check"), &json!(10080), &json!("alice")), "{s}");
}
