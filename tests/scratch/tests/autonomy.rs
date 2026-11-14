//! Autonomy is tested like safety (spec rev 133, rev 140, rev 142). Each release
//! candidate's acceptance runs these eight scenarios end to end, and a release
//! that fails one does not ship:
//!
//! 1. a chat request any chat agent could do is done in the chat, with no
//!    task and no approval;
//! 2. a schedule fires with nobody present and posts its result in the
//!    conversation it was set up in;
//! 3. a task that fails wakes its agent, which retries and reports;
//! 4. an event from a connected app leads to a suggestion or an action;
//! 5. a review proposes work from what it read;
//! 6. a scheduled run uses sub-agents (owner, 2026-11-10), and a failing
//!    scheduled run wakes the agent in the conversation it was set up in;
//! 7. connecting an app needs no further approval to use it (spec rev 140);
//! 8. the agent uses its computer with nobody having set one up (spec rev 142,
//!    "One computer, always there").
//!
//! Each drives a real server with the scripted model and fake apps, and
//! asserts on what the person would see. Each also checks that the plumbing
//! stays hidden: the person's tasks, inbox and conversations hold only what
//! the scenario is about. The proactive loop is on, as it ships, so its
//! background reviews run beside every scenario and must stay out of sight.
//!
//! Run as acceptance (`ci/acceptance.sh`), never on a push.

use magnus_invariants::{mcp, *};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[path = "autonomy/scheduled_sub_agents.rs"]
mod scheduled_sub_agents;

#[path = "autonomy/own_computer.rs"]
mod own_computer;

const CHAT: &str = "talking with the person you work for in a chat";
const REVIEW: &str = "reviewing one person's state";
const PASS: &str = "You keep the person's long-term memory";
const TASK: &str = "A person gave you a task in plain language";
const AUTO_REVIEW: &str = "You are Auto Review";
const CLASSIFIER: &str = "You scan outgoing content for sensitive data";
/// The start of every outcome turn's text: the server's words, not the person's.
const OUTCOME: &str = "This turn is not a message from the person: the server started it because a task";

/// Long enough for a failed task, three attempts and their outcome turns.
const RETRIES_PATIENCE: Duration = Duration::from_secs(300);
/// A daily schedule due in two minutes, its run and its outcome turn.
const SCHEDULE_PATIENCE: Duration = Duration::from_secs(240);
/// A push inside the 30-second gap waits for the sweep after it.
const PUSH_PATIENCE: Duration = Duration::from_secs(150);

/// Which of the server's runs a request belongs to.
#[derive(Debug, PartialEq)]
enum Run {
    /// A chat turn answering the person.
    Chat,
    /// A chat turn the server started because a task ended.
    Outcome,
    Task,
    Review,
    MemoryPass,
    /// The independent check on a boundary action the policy asks about.
    AutoReview,
    /// The scan of what an action carries out, which Auto Review reads.
    Classifier,
    Other,
}

fn system(c: &Conversation) -> String {
    match &c.body["system"] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        other => other.to_string(),
    }
}

/// The text the run was started with: the person's latest words in a chat.
fn said(c: &Conversation) -> String {
    let first = c.messages().first().cloned().unwrap_or_default();
    first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string()
}

/// All the text of the run's first message.
fn opening(c: &Conversation) -> String {
    let first = c.messages().first().cloned().unwrap_or_default();
    first["content"].as_array().into_iter().flatten().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n")
}

fn run_of(c: &Conversation) -> Run {
    let sys = system(c);
    if sys.contains(AUTO_REVIEW) {
        Run::AutoReview
    } else if sys.contains(CLASSIFIER) {
        Run::Classifier
    } else if sys.contains(PASS) {
        Run::MemoryPass
    } else if sys.contains(REVIEW) {
        Run::Review
    } else if sys.contains(CHAT) {
        if said(c).starts_with(OUTCOME) {
            Run::Outcome
        } else {
            Run::Chat
        }
    } else if sys.contains(TASK) {
        Run::Task
    } else {
        Run::Other
    }
}

/// The id of this run's call of `tool`.
fn call_id(c: &Conversation, tool: &str) -> Value {
    let blocks = c.messages().into_iter().flat_map(|m| m["content"].as_array().cloned().unwrap_or_default());
    blocks.into_iter().find(|b| b["type"] == "tool_use" && b["name"] == tool).map(|b| b["id"].clone()).unwrap_or_else(|| panic!("no {tool} call"))
}

fn say(text: &str) -> Reply {
    Reply::Text(text.into())
}

/// What the runs no scenario is about do: reviews find nothing, the memory
/// pass writes nothing (a poll is the kernel's own read), and Auto Review
/// clears what plainly serves the person's request (every action these
/// scenarios take does).
fn background(c: &Conversation) -> Reply {
    match run_of(c) {
        Run::AutoReview => tool("review", json!({ "decision": "allow", "reason": "It does what the person asked." })),
        // Honest about what it sees: a revenue figure is private business data.
        Run::Classifier if said(c).contains("41.7M") => tool("classify", json!({ "sensitive": true, "finding": "a private business figure (Q3 revenue)" })),
        Run::Classifier => tool("classify", json!({ "sensitive": false })),
        Run::Review => complete("Reviewed; nothing to raise."),
        _ => say(""),
    }
}

/// The person's notes: `save` writes one, `list` shows them all.
fn notes() -> mcp::Builder {
    let props = |p: Value| json!({ "type": "object", "properties": p });
    FakeMcp::builder()
        .world("notes", json!([]))
        .honor_idempotency(true)
        .tool("save", "Save a note.", props(json!({ "title": { "type": "string" }, "text": { "type": "string" } })), |w, a, _| {
            if let Some(n) = w["notes"].as_array_mut() {
                n.push(a.clone());
            }
            mcp::ok(json!({ "status": "saved" }))
        })
        .tool("list", "List the notes.", props(json!({})), |w, _, _| mcp::ok(json!({ "notes": w["notes"] })))
}

async fn chat(api: &Api, text: &str) -> (String, String) {
    let (st, v) = api.post("/v1/chat", json!({ "text": text })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let id = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    (id("thread"), id("task_id"))
}

async fn entries(api: &Api, thread: &str) -> Vec<Value> {
    let (st, v) = api.get(&format!("/v1/chat/{thread}")).await;
    assert!(st.is_success(), "{v}");
    v["entries"].as_array().cloned().unwrap_or_default()
}

fn kinds(e: &[Value]) -> Vec<String> {
    e.iter().map(|e| e["kind"].as_str().unwrap_or_default().to_string()).collect()
}

/// The tasks the person sees in their list: the app hides Magnus's own
/// reviews, and the server lists no polls or chat turns.
async fn visible_tasks(api: &Api) -> Vec<Value> {
    api.tasks().await["tasks"].as_array().cloned().unwrap_or_default().into_iter().filter(|t| t["review"] != true && t["poll"] != true).collect()
}

async fn threads(api: &Api) -> BTreeSet<String> {
    let (st, v) = api.get("/v1/chat").await;
    assert!(st.is_success(), "{v}");
    v["threads"].as_array().into_iter().flatten().filter_map(|t| t["thread"].as_str().map(String::from)).collect()
}

/// What the person had before the scenario began: setting up (a grant's
/// conversation, the one Magnus opens about a new app) is not the scenario.
struct Before {
    tasks: BTreeSet<String>,
    items: BTreeSet<String>,
    threads: BTreeSet<String>,
}

fn ids(v: &[Value], key: &str) -> BTreeSet<String> {
    v.iter().filter_map(|x| x[key].as_str().map(String::from)).collect()
}

/// Once the server's first reviews have ended (they list and screen each
/// app's tools), what the person has.
async fn before(api: &Api) -> Before {
    reviews_settled(api).await;
    Before { tasks: ids(&visible_tasks(api).await, "task_id"), items: ids(&api.inbox().await, "id"), threads: threads(api).await }
}

/// The plumbing stays hidden: what the scenario added to the person's tasks,
/// inbox and conversations is exactly what it is about, and nothing else.
async fn only(api: &Api, b: &Before, tasks: &[&str], items: &[&str], convos: &[&str]) {
    let seen: BTreeSet<String> = ids(&visible_tasks(api).await, "task_id").difference(&b.tasks).cloned().collect();
    let want: BTreeSet<String> = tasks.iter().map(|s| s.to_string()).collect();
    assert_eq!(seen, want, "the person's tasks hold more or less than the scenario: {}", api.tasks().await);
    let inbox: Vec<Value> = api.inbox().await.into_iter().filter(|i| !b.items.contains(i["id"].as_str().unwrap_or_default())).collect();
    let mut got: Vec<String> = inbox.iter().map(|i| i["kind"].as_str().unwrap_or_default().to_string()).collect();
    got.sort();
    let mut want: Vec<String> = items.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(got, want, "the person's inbox holds more or less than the scenario: {inbox:?}");
    let seen: BTreeSet<String> = threads(api).await.difference(&b.threads).cloned().collect();
    let want: BTreeSet<String> = convos.iter().map(|s| s.to_string()).collect();
    assert_eq!(seen, want, "the person's conversations hold more or less than the scenario");
}

/// Every run the model was asked about, in order, for a failure's message.
fn story(m: &FakeModel) -> String {
    let cut = |s: &str, n: usize| s.chars().take(n).collect::<String>();
    m.requests()
        .iter()
        .map(|c| {
            let results: Vec<String> = c.last_results().iter().map(|r| format!("{}{}", if r.is_error { "ERR " } else { "" }, cut(&r.text, 300))).collect();
            format!("{:?} turn {} [{}] {:?}", run_of(c), c.turn(), cut(&said(c), 120), results)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Poll `f` until it returns `Some`; on timeout, fail with what the model
/// saw and the person's inbox.
async fn wait<T, F, Fut>(what: &str, patience: Duration, m: &FakeModel, api: &Api, mut f: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let start = std::time::Instant::now();
    loop {
        if let Some(v) = f().await {
            return v;
        }
        if start.elapsed() > patience {
            panic!("timed out after {patience:?} waiting for: {what}\n-- the model saw:\n{}\n-- the inbox: {:?}", story(m), api.inbox().await);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Wait until the person's reviews so far have all ended.
async fn reviews_settled(api: &Api) {
    eventually("the person's reviews ended", PATIENCE, || async {
        let (_, v) = api.get("/v1/tasks").await;
        let reviews: Vec<Value> = v["tasks"].as_array().cloned().unwrap_or_default().into_iter().filter(|t| t["review"] == true).collect();
        (!reviews.is_empty() && reviews.iter().all(|t| t["status"] == "completed" || t["status"] == "failed")).then_some(())
    })
    .await;
}

fn proactive_on() -> Spec {
    Spec::default().proactive(json!({ "on": true }))
}

/// 1. A chat request any chat agent could do is done in the chat: Magnus
/// reads the person's mail and writes a note in their notes app, all in the
/// turn. No task is created and nothing is asked.
#[tokio::test(flavor = "multi_thread")]
async fn a_chat_request_is_done_in_the_chat_with_no_task_and_no_approval() {
    const ASK: &str = "Put what the CFO mailed me into my notes.";
    const ANSWER: &str = "Done: I saved a note \"Q3 numbers\" with the CFO's figure, revenue of 41.7M.";
    let m = FakeModel::start(script(|c| async move {
        if run_of(&c) != Run::Chat {
            return background(&c);
        }
        match c.turn() {
            0 => tool("mail__list_inbox", json!({})),
            1 => tool("notes__save", json!({ "title": "Q3 numbers", "text": "From the CFO: Q3 revenue was 41.7M." })),
            _ => say(ANSWER),
        }
    }))
    .await;
    let (mail, notes) = (mcp::mail().start().await, notes().start().await);
    let spec = proactive_on().app("mail", &mail, "public", "private", &[("list_inbox", "read")]).app(
        "notes",
        &notes,
        "private",
        "private",
        &[("save", "reversible_write"), ("list", "read")],
    );
    let app = Appliance::start(spec, &m).await;
    let alice = app.api("alice");
    let b = before(&alice).await;
    let (thread, turn) = chat(&alice, ASK).await;
    let t = alice.wait_settled(&turn).await;
    assert_eq!(t["status"], "completed", "{t}\n-- the model saw:\n{}\n-- the inbox: {:?}", story(&m), alice.inbox().await);

    // The work is done: the note is in the notes app, and nothing was refused.
    assert_eq!(notes.effects("save"), 1, "the note was not saved");
    assert_eq!(notes.world()["notes"][0]["title"], "Q3 numbers");
    for turn in [1, 2] {
        let r = m.requests().into_iter().find(|c| run_of(c) == Run::Chat && c.turn() == turn).expect("the turn went no further").last_results();
        assert!(r.len() == 1 && !r[0].is_error, "the chat turn's call {turn} was refused or failed: {r:?}");
    }
    // The person sees their message and the answer.
    let e = entries(&alice, &thread).await;
    assert_eq!(kinds(&e), ["message", "answer"], "{e:?}");
    assert_eq!(e[1]["text"], ANSWER);
    assert!(!m.requested_calls().iter().any(|(n, _)| n == "create_task"), "the request was handed to a task");
    // No task, no approval, no notice, and no other conversation.
    only(&alice, &b, &[], &[], &[&thread]).await;
}

/// 2. A schedule fires with nobody present and posts its result. The person
/// sets it up and leaves; nothing touches the server until the run and its
/// outcome turn are over. The result and the agent's report are in the
/// conversation the schedule was set up in (owner, 2026-11-10), and the
/// schedule has no thread of its own.
#[tokio::test(flavor = "multi_thread")]
async fn a_schedule_fires_with_nobody_present_and_posts_its_result() {
    const ASK: &str = "Every morning, tell me what's new in my inbox.";
    const RUN: &str = "Summarise what is new in my inbox.";
    const SUMMARY: &str = "One new message: the CFO sent the Q3 numbers.";
    const REPORT: &str = "This morning's inbox: one message, the CFO's Q3 numbers (revenue 41.7M).";
    let at = due_soon();
    let m = FakeModel::start(script(move |c| {
        let at = at.clone();
        async move {
            match run_of(&c) {
                Run::Chat => match c.turn() {
                    0 => tool("create_schedule", json!({ "title": "Morning inbox", "goal": RUN, "daily_at": at })),
                    _ => say("It is set up."),
                },
                Run::Task => match c.turn() {
                    0 => plan("summarise the inbox"),
                    1 => tool("mail__list_inbox", json!({})),
                    _ => complete(SUMMARY),
                },
                Run::Outcome => say(REPORT),
                _ => background(&c),
            }
        }
    }))
    .await;
    let mail = mcp::mail().start().await;
    let app = Appliance::start(proactive_on().app("mail", &mail, "public", "private", &[("list_inbox", "read")]), &m).await;
    let alice = app.api("alice");
    let b = before(&alice).await;
    let origin = schedule_in_chat(&alice, ASK).await;
    // Nobody is present: no request reaches the server until the run has
    // ended and its outcome turn has answered.
    until_outcome(&m).await;
    let e = in_origin(&m, &alice, &origin, REPORT).await;
    let result = e.iter().find(|e| e["kind"] == "result").unwrap_or_else(|| panic!("no result where the schedule was set up: {e:?}"));
    assert_eq!((result["status"].as_str(), result["summary"].as_str()), (Some("completed"), Some(SUMMARY)), "{result}");
    assert_eq!(e.iter().filter(|e| e["kind"] == "message").count(), 1, "the conversation shows a message nobody wrote: {e:?}");
    assert!(!e.iter().any(|e| e.to_string().contains(OUTCOME)), "the server's turn text is shown to the person: {e:?}");
    // The one run is the person's task, grouped under its schedule, and the
    // schedule has no thread of its own.
    let tasks = visible_tasks(&alice).await;
    assert_eq!(tasks.len(), 1, "{tasks:?}");
    assert_eq!(tasks[0]["group"]["kind"], "schedule", "{tasks:?}");
    let run = tasks[0]["task_id"].as_str().unwrap().to_string();
    assert_eq!(alice.task(&run).await["thread"], origin.as_str(), "the run did not post where the schedule was set up");
    only(&alice, &b, &[&run], &[], &[&origin]).await;
}

/// Wait for the agent's report in the conversation a schedule was set up
/// in: the run's result and the outcome turn's answer post there.
async fn in_origin(m: &FakeModel, api: &Api, origin: &str, report: &str) -> Vec<Value> {
    wait("the report where the schedule was set up", PATIENCE, m, api, || async {
        let e = entries(api, origin).await;
        e.iter().any(|e| e["kind"] == "answer" && e["text"] == report).then_some(e)
    })
    .await
}

/// Ask for a schedule in a new conversation, due at the start of the minute
/// after next. Asking sets it up (spec rev 140): its card shows it set,
/// with nothing to confirm. Returns the conversation.
async fn schedule_in_chat(api: &Api, ask: &str) -> String {
    let (origin, turn) = chat(api, ask).await;
    api.wait_settled(&turn).await;
    let card = entries(api, &origin).await.into_iter().find(|e| e["kind"] == "schedule_card").expect("no schedule card");
    assert_eq!(card["state"], "confirmed", "the schedule waits on a confirmation: {card}");
    origin
}

/// When "every morning" is due: the start of the minute after next, in the
/// server's local time, so it fires once while the test runs.
fn due_soon() -> String {
    (chrono::Local::now() + chrono::Duration::minutes(2)).format("%H:%M").to_string()
}

/// Nobody is present: nothing reaches the server until a run has ended and
/// its outcome turn has been asked.
async fn until_outcome(m: &FakeModel) {
    let start = std::time::Instant::now();
    while !m.requests().iter().any(|c| run_of(c) == Run::Outcome) {
        assert!(start.elapsed() < SCHEDULE_PATIENCE, "the schedule did not run and report:\n{}", story(m));
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// 3. A task that fails wakes its agent, which retries within the authority
/// the person's organisation gave (spec rev 147: no grant to propose), and reports once the retry limit is reached. The outcome comes
/// back as a turn in the conversation that started the task; the person
/// asked once and approved nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_failing_task_wakes_its_agent_which_retries_and_reports() {
    const ASK: &str = "Mark CRM record 42 done, it was finished yesterday.";
    const REASON: &str = "The CRM took the change and then reverted it: record 42 is locked by another user.";
    const REPORT: &str = "I tried three times to mark record 42 done, and each time the CRM reverted it because another user holds a lock on it. I need you to release that lock, or tell me who to ask.";
    // The organisation lets Magnus change CRM records without asking.
    let permits = org_policy("");
    let m = FakeModel::start(script(move |c| async move {
        let retry = |title: &str| tool("create_task", json!({ "title": title, "goal": "Mark CRM record 42 done; check the lock first." }));
        match run_of(&c) {
            Run::Chat => match c.turn() {
                0 => tool("create_task", json!({ "title": "Close CRM record 42", "goal": "Mark CRM record 42 done." })),
                _ => say("On it. I'll tell you here how it went."),
            },
            Run::Task => match c.turn() {
                0 => tool("crm__update_record", json!({ "id": "42", "fields": { "status": "done" } })),
                _ => fail(REASON),
            },
            Run::Outcome => {
                let text = said(&c);
                match (c.turn(), text.contains("start no new task")) {
                    (0, false) => retry(if text.contains("retry 1 of 2") { "Close CRM record 42, second try" } else { "Close CRM record 42, third try" }),
                    (0, true) => retry("Close CRM record 42, fourth try"),
                    (_, false) => say("It failed: the record was locked. I'm trying again."),
                    (_, true) => say(REPORT),
                }
            }
            _ => background(&c),
        }
    }))
    .await;
    let crm = mcp::crm().start().await;
    let spec = proactive_on().app("crm", &crm, "private", "principal", &[("get_record", "read"), ("update_record", "reversible_write")]);
    let app = Appliance::start(spec, &m).await;
    let alice = app.api("alice");
    assert!(alice.put_policy("org", &permits).await.is_success());
    let b = before(&alice).await;

    // The person asks once.
    let (thread, turn) = chat(&alice, ASK).await;
    alice.wait_settled(&turn).await;
    let e = wait("the agent's report after the last retry", RETRIES_PATIENCE, &m, &alice, || async {
        let e = entries(&alice, &thread).await;
        e.iter().any(|e| e["kind"] == "answer" && e["text"] == REPORT).then_some(e)
    })
    .await;

    // What the person sees: their message once, each attempt's failure, and
    // the agent's word after each; never the server's turn text.
    let count = |k: &str| e.iter().filter(|x| x["kind"] == k).count();
    assert_eq!(count("message"), 1, "the thread shows a message the person did not write: {e:?}");
    let results: Vec<&Value> = e.iter().filter(|x| x["kind"] == "result").collect();
    assert_eq!(results.len(), 3, "the first attempt and two retries: {e:?}");
    assert!(results.iter().all(|r| r["status"] == "failed" && r["summary"] == REASON), "{results:?}");
    assert_eq!(count("answer"), 4, "the first answer and one per outcome: {e:?}");
    assert!(!e.iter().any(|x| x.to_string().contains(OUTCOME)), "the server's turn text is shown to the person: {e:?}");

    // At the limit the agent may start nothing.
    let last = m.requests().into_iter().find(|c| run_of(c) == Run::Outcome && c.turn() == 1 && said(c).contains("start no new task")).expect("no turn at the limit");
    let r = last.last_results();
    assert!(r.len() == 1 && r[0].is_error && r[0].text.contains("retried 2 times"), "a retry past the limit was not refused: {r:?}");

    // Every attempt's change went ahead with no approval.
    let tasks = visible_tasks(&alice).await;
    assert_eq!(tasks.len(), 3, "{tasks:?}");
    // Each retry is listed under the person's own message, not the server's turn.
    assert!(tasks.iter().all(|t| t["group"]["title"] == ASK && t["group"]["thread"] == thread.as_str()), "a retry is grouped under the server's turn text: {tasks:?}");
    let ids: Vec<String> = tasks.iter().map(|t| t["task_id"].as_str().unwrap().to_string()).collect();
    for id in &ids {
        let t = alice.task(id).await;
        assert_eq!((t["status"].as_str(), t["thread"].as_str()), (Some("failed"), Some(thread.as_str())), "{t}");
    }
    assert_eq!(crm.effects("update_record"), 3, "each attempt changes the record once");
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    only(&alice, &b, &refs, &[], &[&thread]).await;
}

/// The person's address for an app's pushes.
async fn push_address(app: &Appliance, api: &Api, connector: &str) -> String {
    let (st, v) = api.get(&format!("/v1/connectors/{connector}/push")).await;
    assert!(st.is_success(), "{v}");
    let path = v["path"].as_str().unwrap_or_default();
    assert!(path.starts_with("/v1/push/"), "no push path: {v}");
    format!("{}{path}", app.base)
}

async fn push(url: &str) {
    let r = reqwest::Client::new().post(url).json(&json!({ "value": [{ "changeType": "created" }] })).send().await;
    assert_eq!(r.as_ref().map(|r| r.status().as_u16()).ok(), Some(202), "the push was not taken: {r:?}");
}

/// 4. An event from a connected app leads to a suggestion. The mail app
/// pushes that something changed; the server reads the inbox at once, finds
/// a new message, and wakes a review, which reads it and puts a proposal to
/// the person citing it. Nobody asked. The message is outside text, so the
/// review may not act on it alone (the Rule of Two refuses a background
/// change after untrusted and private reads, and asks no one): it suggests.
#[tokio::test(flavor = "multi_thread")]
async fn an_event_from_a_connected_app_leads_to_a_suggestion() {
    const TITLE: &str = "Move the call with Dana to Thursday 15:00";
    let woke = Arc::new(Mutex::new(Vec::<String>::new()));
    let w = woke.clone();
    let m = FakeModel::start(script(move |c| {
        let w = w.clone();
        async move {
            if run_of(&c) != Run::Review || !opening(&c).contains("new in mail") {
                return background(&c);
            }
            if c.turn() == 0 {
                w.lock().unwrap().push(opening(&c));
            }
            match c.turn() {
                0 => tool("mail__list_inbox", json!({})),
                1 => tool(
                    "suggest",
                    json!({
                        "title": TITLE, "goal": "Reply to Dana (dana@vendor.example) that Thursday 15:00 works, and move the call in the calendar.",
                        "permissions": ["Sends one email to Dana", "Changes one calendar event"],
                        "evidence": [{ "call": call_id(&c, "mail__list_inbox"), "shows": "Dana's new message asks to move the call to Thursday 15:00." }],
                    }),
                ),
                _ => complete("Proposed answering Dana."),
            }
        }
    }))
    .await;
    let mail = mcp::mail().start().await;
    let spec = proactive_on()
        .app("mail", &mail, "public", "private+untrusted", &[("list_inbox", "read")])
        .watch("mail", "list_inbox", "/messages", "subject")
        .access("mail", &["finance"]);
    let app = Appliance::start(spec, &m).await;
    let bob = app.api("bob");
    let b = before(&bob).await;

    // The app pushes: the server reads the inbox at once, and the first
    // read only learns what is there.
    let url = push_address(&app, &bob, "mail").await;
    push(&url).await;
    wait("the push's read", PATIENCE, &m, &bob, || async { (!mail.calls_to("list_inbox").is_empty()).then_some(()) }).await;
    assert!(woke.lock().unwrap().is_empty(), "what was already there woke a review");
    // A message arrives, and the app pushes again.
    mail.set_world(
        "inbox",
        json!([
            { "from": "cfo@corp.example", "subject": "Q3 numbers", "body": "Confidential: revenue 41.7M" },
            { "from": "dana@vendor.example", "subject": "Contract call", "body": "Can we move our call to Thursday 15:00?" },
        ]),
    );
    push(&url).await;
    let item =
        wait("the review's suggestion", PUSH_PATIENCE, &m, &bob, || async { bob.inbox().await.into_iter().find(|i| i["kind"] == "suggestion" && i["status"] == "open") }).await;

    let prompts = woke.lock().unwrap().clone();
    assert!(prompts.len() == 1 && prompts[0].contains("1 new in mail"), "the review was not woken by the new message: {prompts:?}");
    assert_eq!(item["details"]["title"], TITLE, "{item}");
    assert_eq!(item["details"]["evidence"][0]["tool"], "mail__list_inbox", "the suggestion does not cite the read: {item}");
    let read = m.requests().into_iter().find(|c| run_of(c) == Run::Review && c.turn() == 1 && opening(c).contains("new in mail")).expect("no read");
    assert!(read.last_results().iter().any(|r| r.text.contains("Thursday 15:00")), "the review did not read the new message");
    // The push and its read are the server's own work, never the person's.
    only(&bob, &b, &[], &["suggestion"], &[]).await;
}

/// 5. A review proposes work grounded in what it read. Magnus's own review
/// reads the person's inbox, finds a request with a deadline, and suggests a
/// task citing that read. The suggestion waits in the inbox; nothing starts.
#[tokio::test(flavor = "multi_thread")]
async fn a_review_proposes_work_from_what_it_read() {
    const TITLE: &str = "Send Dana the signed contract";
    let proposed = Arc::new(AtomicBool::new(false));
    let p = proposed.clone();
    let m = FakeModel::start(script(move |c| {
        let p = p.clone();
        async move {
            // The first review that can read the inbox proposes; every other run is quiet.
            let reads = c.offered_tools().iter().any(|t| t == "mail__list_inbox");
            if run_of(&c) != Run::Review || !reads || (c.turn() == 0 && p.swap(true, Ordering::SeqCst)) {
                return background(&c);
            }
            match c.turn() {
                0 => tool("mail__list_inbox", json!({})),
                1 => tool(
                    "suggest",
                    json!({
                        "title": TITLE, "goal": "Find the signed contract and send it back to Dana (dana@vendor.example) before Friday, as she asked.",
                        "permissions": ["Reads your mail and files", "Sends one email to Dana"],
                        "evidence": [{ "call": call_id(&c, "mail__list_inbox"), "shows": "Dana's email asks for the signed contract by Friday." }],
                    }),
                ),
                _ => complete("Proposed sending Dana the signed contract."),
            }
        }
    }))
    .await;
    let mail = mcp::mail()
        .world("inbox", json!([{ "from": "dana@vendor.example", "subject": "Signed contract?", "body": "Could you send the signed contract back by Friday?" }]))
        .start()
        .await;
    let spec = proactive_on().app("mail", &mail, "public", "private+untrusted", &[("list_inbox", "read")]).access("mail", &["finance"]);
    let app = Appliance::start(spec, &m).await;
    let bob = app.api("bob");

    // The scenario starts with the server: its first review is the one under test.
    let b = Before { tasks: BTreeSet::new(), items: BTreeSet::new(), threads: BTreeSet::new() };
    let item = wait("the review's proposal", PATIENCE, &m, &bob, || async { bob.inbox().await.into_iter().find(|i| i["kind"] == "suggestion" && i["status"] == "open") }).await;
    let d = &item["details"];
    assert_eq!(d["title"], TITLE, "{item}");
    // Grounded: its evidence is the read the review made, and that read
    // showed the request it proposes to answer.
    assert_eq!(d["evidence"][0]["tool"], "mail__list_inbox", "the evidence is not what the review read: {item}");
    let after = m.requests().into_iter().find(|c| run_of(c) == Run::Review && c.turn() == 1 && c.offered_tools().iter().any(|t| t == "mail__list_inbox")).expect("no read");
    assert!(after.last_results().iter().any(|r| r.text.contains("signed contract back by Friday")), "the review proposed without reading the request");
    let review = item["task_id"].as_str().or(item["task"].as_str()).unwrap_or_else(|| panic!("the proposal names no review: {item}"));
    assert_eq!(bob.task(review).await["review"], true, "the proposal did not come from a review");
    // Nothing started, and the person sees only the proposal.
    only(&bob, &b, &[], &["suggestion"], &[]).await;
}

/// Connect `app` as the desktop app does (spec rev 140): its one connect
/// screen, accepted, then the person's own sign-in. The screen accepted.
async fn connect(api: &Api, app: &str) -> Value {
    let terms = magnus_scratch::accept_connect_screen(api, app).await;
    let (st, v) = api.post(&format!("/v1/connectors/{app}/sign-in"), json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
    assert!(st.is_success(), "{v}");
    let (code, state) = magnus_scratch::FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap_or_default()).await;
    assert_eq!(api.post(&format!("/v1/connectors/{app}/sign-in/complete"), json!({ "state": state, "code": code })).await.0, 204);
    terms
}

/// 7. Connecting an app needs no further approval to use it (spec rev 140,
/// "Connecting an app grants its use"). The organisation adds a calendar by
/// its address, deciding nothing about its tools; the person connects it
/// through its one screen and signs in. A chat request then reads the
/// calendar and adds an event to it: nothing is asked, no tool is reviewed,
/// and no approval reaches the person, before the request or during it.
#[tokio::test(flavor = "multi_thread")]
async fn connecting_an_app_needs_no_further_approval_to_use_it() {
    const ASK: &str = "What's on my calendar? Put lunch with Bob on it for Friday.";
    const ANSWER: &str = "Your calendar was clear, so I added \"Lunch with Bob\" on Friday.";
    let m = FakeModel::start(script(|c| async move {
        if run_of(&c) != Run::Chat || !said(&c).contains(ASK) {
            return background(&c);
        }
        match c.turn() {
            0 => tool("calendar__list_events", json!({})),
            1 => tool("calendar__create_event", json!({ "title": "Lunch with Bob, Friday" })),
            _ => say(ANSWER),
        }
    }))
    .await;
    let cal = magnus_scratch::FakeOAuthApp::start(3600).await;
    let app = Appliance::start(proactive_on(), &m).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/connectors", json!({ "name": "calendar", "url": cal.url, "audience": "private", "label": "private" })).await;
    assert!(st.is_success(), "{v}");
    let b = before(&alice).await;

    // The one screen, in plain words, with nothing to decide per tool.
    let terms = connect(&alice, "calendar").await;
    for part in ["reads", "changes", "risks"] {
        assert!(terms[part].is_array(), "the connect screen has no {part}: {terms}");
    }
    assert!(terms["risks"].to_string().contains("Magnus can use all of it"), "the screen does not say what connecting grants: {terms}");
    // The conversation Magnus opens about a newly connected app is not the scenario.
    let connected = before(&alice).await;

    let (thread, turn) = chat(&alice, ASK).await;
    let t = alice.wait_settled(&turn).await;
    assert_eq!(t["status"], "completed", "{t}\n-- the model saw:\n{}\n-- the inbox: {:?}", story(&m), alice.inbox().await);
    // Both tools ran at once: the read, and the write the request needs.
    for turn in [1, 2] {
        let r = m.requests().into_iter().find(|c| run_of(c) == Run::Chat && said(c).contains(ASK) && c.turn() == turn).expect("the turn went no further").last_results();
        assert!(r.len() == 1 && !r[0].is_error, "the chat turn's call {turn} was refused or failed: {r:?}\n{}", story(&m));
    }
    let made: Vec<String> = cal.state.lock().unwrap().calls.iter().map(|(n, _)| n.clone()).collect();
    assert!(made.contains(&"list_events".to_string()) && made.contains(&"create_event".to_string()), "the calendar was not used: {made:?}");
    let e = entries(&alice, &thread).await;
    assert_eq!(kinds(&e), ["message", "answer"], "{e:?}");
    // Nothing reached the person from connecting to the answer: no tool
    // review, no card for the app, no approval, no task.
    let reached: Vec<Value> = alice.inbox().await.into_iter().filter(|i| !b.items.contains(i["id"].as_str().unwrap_or_default())).collect();
    assert!(reached.is_empty(), "connecting and using the app put something before the person: {reached:?}");
    only(&alice, &connected, &[], &[], &[&thread]).await;
}
