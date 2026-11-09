//! Task proposals (behavior spec, Suggestions and proposals; behavior.md
//! step 4; #254). In chat Magnus proposes one task with `propose_task`: a
//! card with what it noticed and where, the outcome, why now, and what the
//! task needs (permissions, an estimated cost, any input), refused when a
//! field is missing, the estimate is out of bounds, or its evidence points
//! at nothing real. The person approves it (the task starts for them and
//! spends at most the estimate) or dismisses it with a reason, which later
//! runs read in the ledger; a dismissed one comes back only with what
//! changed. It is not put off.

use magnus_invariants::*;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

const CHAT: &str = "talking with the person you work for in a chat";
const PASS: &str = "You keep the person's long-term memory";
const LEDGER: &str = "# Your suggestions to this person and their answers";
const TITLE: &str = "Chase the overdue invoice";
const TEXT: &str = "Draft a reminder to Acme about invoice 1042, overdue by two weeks (task-prop-31).";
const OUTCOME: &str = "A reminder ready to send (outcome-prop-8).";
const REASON: &str = "Acme paid yesterday (why-dis-57).";
const ESTIMATE: f64 = 0.75;

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

/// The first card id the ledger in the system context names.
fn ledger_card(sys: &str) -> String {
    let at = sys.find(LEDGER).and_then(|l| sys[l..].find("card_").map(|i| l + i)).unwrap_or_else(|| panic!("no card in the ledger: {sys}"));
    sys[at..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect()
}

/// A complete proposal citing this turn's `list_tasks` call, with `extra`
/// set over it (null removes a field).
fn proposal(c: &Conversation, extra: Value) -> Value {
    let mut v = json!({
        "title": TITLE, "text": TEXT,
        "noticed": { "what": "Invoice 1042 to Acme is two weeks overdue", "where": "your Tasks" },
        "outcome": OUTCOME, "why_now": "Their payment run is on Friday.",
        "needs": { "permissions": ["Reads your invoices", "Drafts an email; sends nothing"], "estimated_cost_usd": ESTIMATE, "input": "Who at Acme to write to" },
        "evidence": [{ "call": read_call(c), "shows": "The invoice task is still open." }],
    });
    for (k, x) in extra.as_object().cloned().unwrap_or_default() {
        if x.is_null() {
            v.as_object_mut().unwrap().remove(&k);
        } else {
            v[k] = x;
        }
    }
    v
}

/// Chat turns follow `chat` (what the person said, the turn, the request);
/// the memory pass writes nothing; a task states a contract and ends.
fn model(chat: impl Fn(&str, usize, &Conversation) -> Reply + Send + Sync + 'static) -> model::Script {
    let chat = Arc::new(chat);
    script(move |c| {
        let chat = chat.clone();
        async move {
            let sys = system(&c);
            if sys.contains(PASS) {
                return Reply::Text(String::new());
            }
            if !sys.contains(CHAT) {
                return match c.turn() {
                    0 => contract_unchecked("do the proposed work", "it is done"),
                    _ => fail("done"),
                };
            }
            chat(&said(&c), c.turn(), &c)
        }
    })
}

/// Read the person's tasks, then propose with that call as evidence.
fn read_then_propose(turn: usize, c: &Conversation, extra: Value) -> Reply {
    match turn {
        0 => tool("list_tasks", json!({})),
        1 => tool("propose_task", proposal(c, extra)),
        _ => Reply::Text("The card awaits you.".into()),
    }
}

async fn chat(api: &Api, text: &str) -> (String, String) {
    let (st, v) = api.post("/v1/chat", json!({ "text": text })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let task = v["task_id"].as_str().unwrap().to_string();
    api.wait_settled(&task).await;
    (v["thread"].as_str().unwrap_or_default().to_string(), task)
}

async fn entries(api: &Api, thread: &str) -> Vec<Value> {
    let (st, v) = api.get(&format!("/v1/chat/{thread}")).await;
    assert!(st.is_success(), "{v}");
    v["entries"].as_array().cloned().unwrap_or_default()
}

/// Wait until the thread has an entry matching `f`.
async fn wait_entry(api: &Api, thread: &str, f: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..600 {
        if let Some(e) = entries(api, thread).await.into_iter().find(|e| f(e)) {
            return e;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("no such entry in the thread: {:?}", entries(api, thread).await);
}

fn card_of(e: &Value) -> bool {
    e["kind"] == "proposal_card"
}

async fn decide(api: &Api, card: &str, body: Value) -> (u16, Value) {
    let (st, v) = api.post(&format!("/v1/chat/cards/{card}"), body).await;
    (st.as_u16(), v)
}

/// The request the model was given after the call it made in the turn
/// answering `text`, at `turn`.
fn request(m: &FakeModel, text: &str, turn: usize) -> Conversation {
    m.requests().into_iter().find(|c| c.turn() == turn && said(c) == text && system(c).contains(CHAT)).unwrap_or_else(|| panic!("no request at turn {turn} for {text:?}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_proposal_citing_what_it_read_becomes_a_card_and_approving_it_starts_one_task_within_the_estimate() {
    let m = FakeModel::start(model(|_, turn, c| read_then_propose(turn, c, json!({})))).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (thread, _) = chat(&alice, "Anything I should deal with?").await;
    let card = wait_entry(&alice, &thread, card_of).await;
    assert_eq!((&card["state"], &card["title"], &card["text"], &card["outcome"]), (&json!("open"), &json!(TITLE), &json!(TEXT), &json!(OUTCOME)), "{card}");
    assert_eq!(card["noticed"], json!({ "what": "Invoice 1042 to Acme is two weeks overdue", "where": "your Tasks" }), "{card}");
    assert_eq!(card["why_now"], "Their payment run is on Friday.", "{card}");
    assert_eq!(card["needs"]["permissions"], json!(["Reads your invoices", "Drafts an email; sends nothing"]), "{card}");
    assert_eq!((card["needs"]["estimated_cost_usd"].as_f64(), &card["needs"]["input"]), (Some(ESTIMATE), &json!("Who at Acme to write to")), "{card}");
    assert_eq!(card["evidence"][0]["tool"], "list_tasks", "the evidence is not resolved to what was read: {card}");
    assert_eq!(card["scope"], "Reads your invoices; Drafts an email; sends nothing", "{card}");
    assert_eq!(card["budget"]["max_usd"].as_f64(), Some(ESTIMATE), "the card's budget is not the estimate: {card}");
    assert_eq!(card["acts_for"], "alice");
    let id = card["card"].as_str().unwrap();
    let before = alice.tasks().await["tasks"].as_array().unwrap().len();

    // Only the person it was put to decides it; approving it starts one task for them.
    assert_eq!(decide(&app.api("bob"), id, json!({ "decision": "approve" })).await.0, 404, "someone else approved the card");
    let (st, v) = decide(&alice, id, json!({ "decision": "approve" })).await;
    assert_eq!(st, 200, "{v}");
    let task = v["task"].as_str().expect("no task started").to_string();
    assert_eq!(decide(&alice, id, json!({ "decision": "approve" })).await.0, 409, "a card was approved twice");
    assert_eq!(decide(&alice, id, json!({ "decision": "dismiss", "reason": REASON })).await.0, 409, "an approved card was dismissed");
    let t = alice.wait_settled(&task).await;
    assert_eq!(t["person"], "alice", "the task does not act for the person: {t}");
    assert_eq!(alice.tasks().await["tasks"].as_array().unwrap().len(), before + 1, "approving it twice started two tasks");
    // It spends at most the cost the person approved, not the appliance's limit.
    let t = alice.task(&task).await;
    assert_eq!(t["budget"]["max_usd"].as_f64(), Some(ESTIMATE), "the task's budget is not the estimate: {t}");
    let card = wait_entry(&alice, &thread, card_of).await;
    assert_eq!((&card["state"], &card["started"]), (&json!("confirmed"), &json!(task)), "{card}");
    // Its result posts to the thread.
    let result = wait_entry(&alice, &thread, |e| e["kind"] == "result").await;
    assert_eq!(result["task_id"], task.as_str());
    let theirs = |c: &Conversation| !system(c).contains(CHAT) && !system(c).contains(PASS);
    assert!(m.requests().iter().any(|c| theirs(c) && c.text().contains("task-prop-31")), "the task does not carry the proposal's text");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_proposal_missing_a_field_or_with_a_bad_estimate_or_evidence_is_refused() {
    let m = FakeModel::start(model(|_, turn, c| {
        let bad = |extra: Value| tool("propose_task", proposal(c, extra));
        let needs = |cost: Value| json!({ "needs": { "permissions": ["Reads your invoices"], "estimated_cost_usd": cost } });
        match turn {
            0 => tool("list_tasks", json!({})),
            1 => bad(json!({ "why_now": null })),
            2 => bad(json!({ "noticed": { "what": "An overdue invoice", "where": " " } })),
            3 => bad(json!({ "needs": { "permissions": [], "estimated_cost_usd": ESTIMATE } })),
            4 => bad(needs(json!(0))),
            5 => bad(needs(json!(51.0))),
            6 => bad(json!({ "evidence": [{ "call": "toolu_never_made", "shows": "x" }] })),
            _ => Reply::Text("Done.".into()),
        }
    }))
    .await;
    // The appliance's limit per task is $50 (Spec::default).
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let text = "Anything I should deal with?";
    let (thread, _) = chat(&alice, text).await;
    for (turn, why) in [(2, "why_now"), (3, "noticed"), (4, "needs.permissions"), (5, "estimated_cost_usd"), (6, "estimated_cost_usd"), (7, "evidence item 1")] {
        let r = request(&m, text, turn).last_results();
        assert!(r[0].is_error && r[0].text.starts_with("No card") && r[0].text.contains(why) && r[0].text.contains("will not help"), "turn {turn}: {r:?}");
    }
    assert!(!entries(&alice, &thread).await.iter().any(card_of), "a card was made");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dismissal_needs_a_reason_which_later_runs_read_in_the_ledger_and_a_proposal_is_not_put_off() {
    let m = FakeModel::start(model(|said, turn, c| match said {
        s if s.contains("deal with") => read_then_propose(turn, c, json!({})),
        _ => Reply::Text("Understood.".into()),
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (thread, _) = chat(&alice, "Anything I should deal with?").await;
    let id = wait_entry(&alice, &thread, card_of).await["card"].as_str().unwrap().to_string();
    let before = alice.tasks().await["tasks"].as_array().unwrap().len();
    let (st, v) = decide(&alice, &id, json!({ "decision": "later" })).await;
    assert!(st == 400 && v["error"].as_str().is_some_and(|e| e.contains("approved or dismissed")), "a proposal was put off: {st} {v}");
    assert_eq!(decide(&alice, &id, json!({ "decision": "dismiss" })).await.0, 400, "dismissed without a reason");
    assert_eq!(decide(&alice, &id, json!({ "decision": "dismiss", "reason": "  " })).await.0, 400, "dismissed with a blank reason");
    assert_eq!(decide(&app.api("bob"), &id, json!({ "decision": "dismiss", "reason": REASON })).await.0, 404, "someone else dismissed the card");
    let (st, v) = decide(&alice, &id, json!({ "decision": "dismiss", "reason": REASON })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(decide(&alice, &id, json!({ "decision": "approve" })).await.0, 409, "a dismissed card was approved");
    let card = wait_entry(&alice, &thread, card_of).await;
    assert_eq!((&card["state"], &card["reason"]), (&json!("declined"), &json!(REASON)), "{card}");
    assert_eq!(alice.tasks().await["tasks"].as_array().unwrap().len(), before, "a dismissed proposal started its task");

    // The thread's next turn reads the dismissal; so does her next run elsewhere, in the ledger.
    let text = "Why not?";
    let (st, v) = alice.post("/v1/chat", json!({ "text": text, "thread": thread })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    alice.wait_settled(v["task_id"].as_str().unwrap()).await;
    let conversation = serde_json::to_string(&request(&m, text, 0).messages()).unwrap();
    assert!(conversation.contains("Proposed the task") && conversation.contains("why-dis-57"), "the conversation does not carry the dismissal: {conversation}");
    let text = "How is the week?";
    chat(&alice, text).await;
    let sys = system(&request(&m, text, 0));
    let ledger = &sys[sys.find(LEDGER).unwrap_or_else(|| panic!("no ledger: {sys}"))..];
    let line = ledger.lines().find(|l| l.contains(&format!("{id}: "))).unwrap_or_else(|| panic!("{id} is not in the ledger: {ledger}"));
    assert!(line.contains("you proposed the task") && line.contains(TITLE) && line.contains("outcome-prop-8"), "{line}");
    assert!(line.contains("they dismissed it") && line.contains("why-dis-57"), "{line}");
    assert!(ledger.contains("proposals"), "the ledger does not say it holds proposals: {ledger}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dismissed_proposal_comes_back_only_with_what_changed() {
    let m = FakeModel::start(model(|said, turn, c| match (said, turn) {
        (s, _) if s.contains("deal with") => read_then_propose(turn, c, json!({})),
        (_, 0) => tool("list_tasks", json!({})),
        (_, 1) => tool("propose_task", proposal(c, json!({ "title": "  chase the OVERDUE invoice " }))),
        (_, 2) => tool("propose_task", proposal(c, json!({ "again": ledger_card(&system(c)) }))),
        (_, 3) => tool("propose_task", proposal(c, json!({ "again": ledger_card(&system(c)), "changed": "Their payment bounced (chg-prop-4)." }))),
        _ => Reply::Text("The card awaits you.".into()),
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (thread, _) = chat(&alice, "Anything I should deal with?").await;
    let id = wait_entry(&alice, &thread, card_of).await["card"].as_str().unwrap().to_string();
    assert_eq!(decide(&alice, &id, json!({ "decision": "dismiss", "reason": REASON })).await.0, 200);

    let text = "Look at the invoices again.";
    let (thread, _) = chat(&alice, text).await;
    // The same title, without again: refused, naming the dismissed card.
    let r = request(&m, text, 2).last_results();
    assert!(r[0].is_error && r[0].text.contains(&id) && r[0].text.contains("changed"), "{r:?}");
    // With again but not what changed: refused.
    let r = request(&m, text, 3).last_results();
    assert!(r[0].is_error && r[0].text.contains("changed"), "{r:?}");
    // With both: a card that shows what changed.
    let card = wait_entry(&alice, &thread, card_of).await;
    assert_eq!((&card["again"], &card["changed"], &card["state"]), (&json!(id), &json!("Their payment bounced (chg-prop-4)."), &json!("open")), "{card}");
}
