//! Automation suggestions (behavior spec, Suggestions and proposals;
//! behavior.md step 4; #253). In chat Magnus suggests something it could
//! do with `suggest`: a card with a title, plan, evidence and permissions,
//! refused when its evidence does not point at something real. The person
//! accepts it (a task or a schedule starts), puts it off ("Not now"), or
//! declines it with a reason. Every card and its answer are in the ledger
//! that later runs of theirs read; a declined one comes back only with what
//! changed. A schedule the person asks for is not a suggestion: it is set
//! up at once (spec rev 140), and is not put to them again.

use magnus_invariants::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const CHAT: &str = "talking with the person you work for in a chat";
const PASS: &str = "You keep the person's long-term memory";
const LEDGER: &str = "# Your suggestions to this person and their answers";
const TITLE: &str = "Weekly invoice digest";
const PLAN: &str = "Every week, read the new invoices and summarise them (plan-sug-17).";
const REASON: &str = "My accountant does this already (why-dec-42).";

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

/// The id of the call whose result the model was just given.
fn last_call(c: &Conversation) -> String {
    c.last_results().first().map(|r| r.tool_use_id.clone()).unwrap_or_default()
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

fn suggestion(evidence: Value, extra: Value) -> Value {
    let mut v = json!({ "title": TITLE, "goal": PLAN, "evidence": evidence, "permissions": ["Reads your invoices", "Sends nothing"] });
    for (k, x) in extra.as_object().cloned().unwrap_or_default() {
        v[k] = x;
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
                    0 => contract_unchecked("do the suggested work", "it is done"),
                    _ => fail("done"),
                };
            }
            chat(&said(&c), c.turn(), &c)
        }
    })
}

/// Read the person's tasks, then suggest with that call as evidence.
fn read_then_suggest(turn: usize, c: &Conversation, extra: Value) -> Reply {
    match turn {
        0 => tool("list_tasks", json!({})),
        1 => tool("suggest", suggestion(json!([{ "call": last_call(c), "shows": "You ask for an invoice summary every week." }]), extra)),
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
async fn a_suggestion_citing_what_it_read_becomes_a_card_and_accepting_it_starts_a_task_or_a_schedule() {
    let m = FakeModel::start(model(|said, turn, c| match said {
        s if s.contains("each morning") => read_then_suggest(turn, c, json!({ "title": "Morning invoice check", "daily_at": "07:30", "max_runs": 5 })),
        _ => read_then_suggest(turn, c, json!({})),
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (thread, _) = chat(&alice, "What could you take off my plate?").await;
    let card = wait_entry(&alice, &thread, |e| e["kind"] == "suggestion_card").await;
    assert_eq!((&card["state"], &card["title"], &card["plan"]), (&json!("open"), &json!(TITLE), &json!(PLAN)), "{card}");
    assert_eq!(card["evidence"][0]["tool"], "list_tasks", "the evidence is not resolved to what was read: {card}");
    assert_eq!(card["permissions"], json!(["Reads your invoices", "Sends nothing"]));
    assert_eq!(card["acts_for"], "alice");
    let id = card["card"].as_str().unwrap();
    let before = alice.tasks().await["tasks"].as_array().unwrap().len();

    // Only the person it was put to decides it; accepting it starts a task for them.
    assert_eq!(decide(&app.api("bob"), id, json!({ "decision": "accept" })).await.0, 404, "someone else accepted the card");
    let (st, v) = decide(&alice, id, json!({ "decision": "accept" })).await;
    assert_eq!(st, 200, "{v}");
    let task = v["task"].as_str().expect("no task started").to_string();
    assert_eq!(decide(&alice, id, json!({ "decision": "accept" })).await.0, 409, "a card was accepted twice");
    let t = alice.wait_settled(&task).await;
    assert_eq!(t["person"], "alice", "the task does not act for the person: {t}");
    assert_eq!(alice.tasks().await["tasks"].as_array().unwrap().len(), before + 1, "accepting it twice started two tasks");
    let card = wait_entry(&alice, &thread, |e| e["kind"] == "suggestion_card").await;
    assert_eq!((&card["state"], &card["started"]), (&json!("confirmed"), &json!(task)), "{card}");
    // Its result posts to the thread.
    let result = wait_entry(&alice, &thread, |e| e["kind"] == "result").await;
    assert_eq!(result["task_id"], task.as_str());
    let theirs = |c: &Conversation| !system(c).contains(CHAT) && !system(c).contains(PASS);
    assert!(m.requests().iter().any(|c| theirs(c) && c.text().contains("plan-sug-17")), "the task does not carry the plan");

    // With timing, accepting it makes the schedule.
    let (thread, _) = chat(&alice, "Check my invoices each morning?").await;
    let card = wait_entry(&alice, &thread, |e| e["kind"] == "suggestion_card").await;
    assert_eq!((&card["daily_at"], &card["max_runs"]), (&json!("07:30"), &json!(5)), "{card}");
    let (_, s) = alice.get("/v1/schedules").await;
    assert!(s["schedules"].as_array().unwrap().is_empty(), "scheduled before it was accepted: {s}");
    let (st, v) = decide(&alice, card["card"].as_str().unwrap(), json!({ "decision": "accept" })).await;
    assert_eq!(st, 200, "{v}");
    let (_, s) = alice.get("/v1/schedules").await;
    let list = s["schedules"].as_array().unwrap();
    assert_eq!(list.len(), 1, "{s}");
    assert_eq!(list[0]["id"], v["schedule"], "{v}");
    let card = wait_entry(&alice, &thread, |e| e["kind"] == "suggestion_card").await;
    assert_eq!((&card["state"], &card["schedule"]), (&json!("confirmed"), &v["schedule"]), "{card}");
}

#[tokio::test(flavor = "multi_thread")]
async fn evidence_that_points_at_nothing_real_is_refused() {
    let bobs = Arc::new(Mutex::new(String::new()));
    let theirs = bobs.clone();
    let m = FakeModel::start(model(move |said, turn, c| {
        let cite = |e: Value| tool("suggest", suggestion(json!([e]), json!({})));
        match (said, turn) {
            ("Cite a call that never was.", 0) => cite(json!({ "call": "toolu_never_made", "shows": "x" })),
            ("Cite a card.", 0) => tool("create_schedule", json!({ "title": "News", "goal": "Summarise the news.", "daily_at": "07:30" })),
            ("Cite a card.", 1) => cite(json!({ "call": last_call(c), "shows": "x" })),
            ("Cite Bob's task.", 0) => cite(json!({ "task": theirs.lock().unwrap().clone(), "shows": "x" })),
            (s, 0) if s.starts_with("Cite my own task") => cite(json!({ "task": s.rsplit(' ').next().unwrap(), "shows": "You asked for this before." })),
            _ => Reply::Text("Done.".into()),
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let (_, bob_task) = chat(&bob, "Hello.").await;
    *bobs.lock().unwrap() = bob_task;

    for (text, why) in [("Cite a call that never was.", "toolu_never_made"), ("Cite a card.", "no tool result"), ("Cite Bob's task.", "no task of this person's")] {
        let (thread, _) = chat(&alice, text).await;
        let turn = if text == "Cite a card." { 2 } else { 1 };
        let r = request(&m, text, turn).last_results();
        assert!(r[0].is_error && r[0].text.contains("evidence item 1") && r[0].text.contains(why) && r[0].text.contains("will not help"), "{text}: {r:?}");
        assert!(!entries(&alice, &thread).await.iter().any(|e| e["kind"] == "suggestion_card"), "{text}: a card was made");
    }
    // A task of her own is real.
    let (_, mine) = chat(&alice, "Hello.").await;
    let text = format!("Cite my own task {mine}");
    let (thread, _) = chat(&alice, &text).await;
    let card = wait_entry(&alice, &thread, |e| e["kind"] == "suggestion_card").await;
    assert_eq!((&card["evidence"][0]["task"], &card["evidence"][0]["title"]), (&json!(mine), &json!("Hello.")), "{card}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_decline_needs_a_reason_which_later_runs_read_in_the_ledger_also_after_a_restart() {
    let m = FakeModel::start(model(|said, turn, c| match said {
        s if s.contains("plate") => read_then_suggest(turn, c, json!({})),
        s if s.contains("Anything else") => read_then_suggest(turn, c, json!({ "title": "Inbox triage" })),
        _ => Reply::Text("Understood.".into()),
    }))
    .await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (thread, _) = chat(&alice, "What could you take off my plate?").await;
    let id = wait_entry(&alice, &thread, |e| e["kind"] == "suggestion_card").await["card"].as_str().unwrap().to_string();
    assert_eq!(decide(&alice, &id, json!({ "decision": "decline" })).await.0, 400, "declined without a reason");
    assert_eq!(decide(&alice, &id, json!({ "decision": "decline", "reason": "  " })).await.0, 400, "declined with a blank reason");
    let (st, v) = decide(&alice, &id, json!({ "decision": "decline", "reason": REASON })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(decide(&alice, &id, json!({ "decision": "later" })).await.0, 409, "a declined card was decided again");
    let card = wait_entry(&alice, &thread, |e| e["kind"] == "suggestion_card").await;
    assert_eq!((&card["state"], &card["reason"]), (&json!("declined"), &json!(REASON)), "{card}");
    // The reason is in the content store, not the log's record.
    let db = app.db.admin().await;
    let rows = db.query("SELECT body::text FROM public.events WHERE kind = 'card_declined'", &[]).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].get::<_, String>(0).contains("why-dec-42"), "the reason is in the log's record");

    // Not now on another card.
    let (thread2, _) = chat(&alice, "Anything else?").await;
    let other = wait_entry(&alice, &thread2, |e| e["kind"] == "suggestion_card").await["card"].as_str().unwrap().to_string();
    let (st, v) = decide(&alice, &other, json!({ "decision": "later" })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(wait_entry(&alice, &thread2, |e| e["kind"] == "suggestion_card").await["state"], "later");

    // Her next run reads both, with their answers; so does one after a restart.
    for text in ["How is the week?", "And now?"] {
        if text == "And now?" {
            app.restart().await;
        }
        let alice = app.api("alice");
        chat(&alice, text).await;
        let sys = system(&request(&m, text, 0));
        let ledger = &sys[sys.find(LEDGER).unwrap_or_else(|| panic!("no ledger: {sys}"))..];
        let line = |card: &str| ledger.lines().find(|l| l.contains(&format!("{card}: "))).unwrap_or_else(|| panic!("{card} is not in the ledger: {ledger}"));
        assert!(line(&id).contains(TITLE) && line(&id).contains("declined") && line(&id).contains("why-dec-42"), "{ledger}");
        assert!(line(&other).contains("not now"), "{ledger}");
        assert!(ledger.contains("again"), "the ledger does not say how a declined card comes back: {ledger}");
    }
    // Bob's runs read none of it.
    chat(&app.api("bob"), "How is the week?").await;
    let bobs = m.requests().into_iter().filter(|c| said(c) == "How is the week?" && c.turn() == 0 && system(c).contains(CHAT)).map(|c| system(&c)).collect::<Vec<_>>();
    assert_eq!(bobs.len(), 2);
    assert_eq!(bobs.iter().filter(|s| s.contains("why-dec-42")).count(), 1, "another person's run read her ledger");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_declined_suggestion_comes_back_only_with_what_changed() {
    let m = FakeModel::start(model(|said, turn, c| {
        // Each try cites what this turn read first, as it must.
        let again = |extra: Value| tool("suggest", suggestion(json!([{ "call": read_call(c), "shows": "You now get twice as many invoices." }]), extra));
        match (said, turn) {
            (s, _) if s.contains("plate") => read_then_suggest(turn, c, json!({})),
            (_, 0) => tool("list_tasks", json!({})),
            (_, 1) => again(json!({ "title": "  weekly INVOICE digest " })),
            (_, 2) => again(json!({ "again": ledger_card(&system(c)) })),
            (_, 3) => again(json!({ "again": ledger_card(&system(c)), "changed": "Your invoices doubled this month (chg-63)." })),
            _ => Reply::Text("The card awaits you.".into()),
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (thread, _) = chat(&alice, "What could you take off my plate?").await;
    let id = wait_entry(&alice, &thread, |e| e["kind"] == "suggestion_card").await["card"].as_str().unwrap().to_string();
    assert_eq!(decide(&alice, &id, json!({ "decision": "decline", "reason": REASON })).await.0, 200);

    let text = "Try again with the invoices.";
    let (thread, _) = chat(&alice, text).await;
    // The same title, without again: refused, naming the declined card.
    let r = request(&m, text, 2).last_results();
    assert!(r[0].is_error && r[0].text.contains(&id) && r[0].text.contains("changed"), "{r:?}");
    // With again but not what changed: refused.
    let r = request(&m, text, 3).last_results();
    assert!(r[0].is_error && r[0].text.contains("changed"), "{r:?}");
    // With both: a card that shows what changed.
    let card = wait_entry(&alice, &thread, |e| e["kind"] == "suggestion_card").await;
    assert_eq!((&card["again"], &card["changed"], &card["state"]), (&json!(id), &json!("Your invoices doubled this month (chg-63)."), &json!("open")), "{card}");
}

/// Review of #256: naming another card of theirs in `again` does not bring
/// back a title they declined, with or without what changed.
#[tokio::test(flavor = "multi_thread")]
async fn a_declined_title_does_not_come_back_by_naming_another_card() {
    let open = Arc::new(Mutex::new(String::new()));
    let other = open.clone();
    let m = FakeModel::start(model(move |said, turn, c| {
        let back = |extra: Value| tool("suggest", suggestion(json!([{ "call": read_call(c), "shows": "You still get invoices." }]), extra));
        match (said, turn) {
            (s, _) if s.contains("plate") => read_then_suggest(turn, c, json!({})),
            (s, _) if s.contains("Anything else") => read_then_suggest(turn, c, json!({ "title": "Inbox triage" })),
            (_, 0) => tool("list_tasks", json!({})),
            (_, 1) => back(json!({ "again": other.lock().unwrap().clone() })),
            (_, 2) => back(json!({ "again": other.lock().unwrap().clone(), "changed": "Nothing really." })),
            _ => Reply::Text("Done.".into()),
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (thread, _) = chat(&alice, "What could you take off my plate?").await;
    let declined = wait_entry(&alice, &thread, |e| e["kind"] == "suggestion_card").await["card"].as_str().unwrap().to_string();
    assert_eq!(decide(&alice, &declined, json!({ "decision": "decline", "reason": REASON })).await.0, 200);
    let (thread, _) = chat(&alice, "Anything else?").await;
    *open.lock().unwrap() = wait_entry(&alice, &thread, |e| e["kind"] == "suggestion_card").await["card"].as_str().unwrap().to_string();

    let text = "Once more with the invoices.";
    let (thread, _) = chat(&alice, text).await;
    for turn in [2, 3] {
        let r = request(&m, text, turn).last_results();
        assert!(r[0].is_error && r[0].text.contains(&declined) && r[0].text.contains("changed"), "turn {turn}: {r:?}");
    }
    assert!(!entries(&alice, &thread).await.iter().any(|e| e["kind"] == "suggestion_card"), "the declined title came back");
}

/// Review of #256: timing that is given is a schedule's and checked as one;
/// only the 0 and "" a model fills in for a field it does not mean are none.
#[tokio::test(flavor = "multi_thread")]
async fn timing_given_is_checked_as_a_schedules() {
    let m = FakeModel::start(model(|_, turn, c| {
        let timed = |extra: Value| tool("suggest", suggestion(json!([{ "call": read_call(c), "shows": "You ask for it weekly." }]), extra));
        match turn {
            0 => tool("list_tasks", json!({})),
            1 => timed(json!({ "every_minutes": -60 })),
            2 => timed(json!({ "until": "2099-01-01T00:00:00Z" })),
            3 => timed(json!({ "every_minutes": 0, "daily_at": " ", "until": "", "max_runs": 0 })),
            _ => Reply::Text("Done.".into()),
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let text = "What could you do regularly?";
    let (thread, _) = chat(&alice, text).await;
    for turn in [2, 3] {
        let r = request(&m, text, turn).last_results();
        assert!(r[0].is_error && r[0].text.contains("every_minutes or daily_at"), "turn {turn}: {r:?}");
    }
    let card = wait_entry(&alice, &thread, |e| e["kind"] == "suggestion_card").await;
    assert_eq!((&card["every_minutes"], &card["daily_at"], &card["until"], &card["max_runs"]), (&Value::Null, &Value::Null, &Value::Null, &Value::Null), "{card}");
}

/// A schedule the person asks for is set up at once: its card shows it set,
/// it cannot be put off or declined as a suggestion, and later turns read it
/// as done at their request, not as a suggestion awaiting an answer.
#[tokio::test(flavor = "multi_thread")]
async fn a_schedule_asked_for_is_set_up_and_not_put_to_the_person_as_a_suggestion() {
    let m = FakeModel::start(model(|said, turn, _| match (said, turn) {
        (s, 0) if s.starts_with("Every") => tool("create_schedule", json!({ "title": s, "goal": "Summarise the news (sched-plan-9).", "daily_at": "07:30" })),
        _ => Reply::Text("It is set up.".into()),
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (thread, _) = chat(&alice, "Every morning, the news.").await;
    let card = wait_entry(&alice, &thread, |e| e["kind"] == "schedule_card").await;
    let (_, s) = alice.get("/v1/schedules").await;
    let list = s["schedules"].as_array().unwrap();
    assert_eq!(list.len(), 1, "{s}");
    assert_eq!((&card["state"], &card["schedule"], &card["plan"]), (&json!("confirmed"), &list[0]["id"], &json!("Summarise the news (sched-plan-9).")), "{card}");
    let id = card["card"].as_str().unwrap().to_string();
    for d in [json!({ "decision": "later" }), json!({ "decision": "decline", "reason": REASON }), json!({ "decision": "confirm" })] {
        assert_eq!(decide(&alice, &id, d.clone()).await.0, 409, "a schedule set up at her request was answered as a card: {d}");
    }
    assert_eq!(wait_entry(&alice, &thread, |e| e["kind"] == "schedule_card").await["state"], "confirmed");
    // The thread's next turn reads it as set up at her request; the ledger of suggestions leaves it out.
    let text = "Thanks.";
    let (st, v) = alice.post("/v1/chat", json!({ "text": text, "thread": thread })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    alice.wait_settled(v["task_id"].as_str().unwrap()).await;
    let c = request(&m, text, 0);
    let conversation = serde_json::to_string(&c.messages()).unwrap();
    assert!(conversation.contains("Set up the schedule") && !conversation.contains("Proposed a schedule"), "the conversation does not read it as set up: {conversation}");
    assert!(!system(&c).contains("you proposed the schedule"), "the schedule is in the ledger of suggestions");
}

/// Accepting an untimed suggestion starts a one-off task: its context says
/// the person approved it for its scope, without calling it a schedule;
/// a schedule's run keeps its own wording (#271).
#[tokio::test(flavor = "multi_thread")]
async fn an_accepted_suggestion_starts_a_task_approved_for_its_scope_not_a_schedule() {
    let m = FakeModel::start(model(|_, turn, c| read_then_suggest(turn, c, json!({})))).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (thread, _) = chat(&alice, "What could you take off my plate?").await;
    let card = wait_entry(&alice, &thread, |e| e["kind"] == "suggestion_card").await;
    let (st, v) = decide(&alice, card["card"].as_str().unwrap(), json!({ "decision": "accept" })).await;
    assert_eq!(st, 200, "{v}");
    let task = v["task"].as_str().expect("no task started").to_string();
    alice.wait_settled(&task).await;
    let theirs = |c: &Conversation| !system(c).contains(CHAT) && !system(c).contains(PASS);
    let asked: Vec<Conversation> = m.requests().into_iter().filter(|c| theirs(c)).collect();
    assert!(!asked.is_empty(), "the task never asked");
    let first = serde_json::to_string(&asked[0].messages()).unwrap();
    assert!(first.contains("approved this task for this scope"), "the task was not told it was approved: {first}");
    assert!(!first.contains("this schedule"), "the one-off task was told it is a schedule: {first}");
}
