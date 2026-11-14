//! The commitment ledger (behavior spec, Commitment recall; behavior.md
//! step 6; #236). What the person and Magnus said they would do, to whom,
//! by when and from where, recognised by Magnus in a conversation or in
//! what it read in an app, and recorded only where its words were really
//! said. The person's open commitments are in every run of theirs, with
//! their taint; overdue is derived from the date; the person sees,
//! corrects, completes, releases and removes theirs, and no one else sees
//! them.

use magnus_invariants::{mcp, *};
use serde_json::{json, Value};

const CHAT: &str = "talking with the person you work for in a chat";
const PASS: &str = "You keep the person's long-term memory";
const SECTION: &str = "# Commitments";

/// Alice's commitment, as she says it.
const DECK: &str = "Sure, I'll send Dana the deck by Friday.";
const DECK_QUOTE: &str = "i'll send dana THE deck";
/// A message whose words promise nothing.
const IDLE: &str = "What a week it has been.";
/// The person asks Magnus to remind them; Magnus's answer is its own commitment.
const ASK_REMIND: &str = "Can you remind me about the invoice?";
const REMIND: &str = "I'll remind you about the invoice on Monday.";
const THANKS: &str = "Thanks, please do.";
/// Each says the deck again, as a second commitment.
const AGAIN: &str = "Again: I'll send Dana the deck by Friday.";
/// A commitment already late when it is made.
const LATE: &str = "I'll file the expense report by January 5, 2020.";
/// The person asks Magnus to look in their mail for what they promised.
const MAIL: &str = "Check my mail for what I promised.";
const MAIL_NO_CALL: &str = "Note what my mail says I promised.";
/// In the mailbox, written by someone else.
const PROMISE: &str = "As agreed on the call, Alice: you will review the Q3 contract by Monday and send Bob your notes.";
const MAIL_QUOTE: &str = "you will review the Q3 contract by Monday";
/// Mark the commitment done, or released, by its id, when told.
const DONE: &str = "I sent the deck. id=";
const OFF: &str = "The report is off. id=";

fn said(c: &Conversation) -> String {
    let first = c.messages().first().cloned().unwrap_or_default();
    first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string()
}

fn system(c: &Conversation) -> String {
    match &c.body["system"] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        other => other.to_string(),
    }
}

fn call_of(c: &Conversation, tool: &str) -> Value {
    let blocks = c.messages().into_iter().flat_map(|m| m["content"].as_array().cloned().unwrap_or_default());
    blocks.into_iter().find(|b| b["type"] == "tool_use" && b["name"] == tool).map(|b| b["id"].clone()).unwrap_or_else(|| panic!("no {tool} call"))
}

/// The word after `id=` in a message.
fn id_in(text: &str) -> String {
    text.split("id=").nth(1).unwrap_or_default().trim().to_string()
}

fn deck() -> Value {
    json!({ "who": "person", "what": "Send Dana the deck", "to": "Dana", "by": "2099-12-31", "quote": DECK_QUOTE })
}

/// Each chat message of the person's drives one turn:
/// a commitment said is recorded, one read in the mail is recorded citing
/// the read (or, without it, refused), and one in no one's words is
/// refused. A task contracts and stops; the memory pass writes nothing.
fn model() -> model::Script {
    script(|c| async move {
        let sys = system(&c);
        if sys.contains(PASS) {
            return Reply::Text(String::new());
        }
        if !sys.contains(CHAT) {
            return match c.turn() {
                0 => plan("do it"),
                _ => fail("done"),
            };
        }
        let text = said(&c);
        let once = |r: Reply| if c.turn() == 0 { r } else { Reply::Text("Noted.".into()) };
        match text.as_str() {
            DECK | AGAIN => once(tool("record_commitment", deck())),
            IDLE => once(tool("record_commitment", json!({ "who": "person", "what": "Fly to Paris", "quote": "I will fly to Paris on Sunday" }))),
            ASK_REMIND => Reply::Text(REMIND.into()),
            THANKS => once(tool(
                "record_commitment",
                json!({ "who": "magnus", "what": "Remind them about the invoice", "by": "2099-12-28", "quote": "I'll remind you about the invoice" }),
            )),
            LATE => {
                once(tool("record_commitment", json!({ "who": "person", "what": "File the expense report", "by": "2020-01-05", "quote": "file the expense report by January 5" })))
            }
            MAIL | MAIL_NO_CALL => match c.turn() {
                0 => tool("mail__list_inbox", json!({})),
                1 => {
                    let mut input = json!({ "who": "person", "what": "Review the Q3 contract", "to": "Bob", "by": "2020-03-02", "quote": MAIL_QUOTE });
                    if text == MAIL {
                        input["call"] = call_of(&c, "mail__list_inbox");
                    }
                    tool("record_commitment", input)
                }
                _ => Reply::Text("Noted.".into()),
            },
            t if t.starts_with(DONE) => once(tool("update_commitment", json!({ "id": id_in(t), "status": "done", "note": "They said they sent it." }))),
            t if t.starts_with(OFF) => once(tool("update_commitment", json!({ "id": id_in(t), "status": "released" }))),
            _ => Reply::Text("Noted.".into()),
        }
    })
}

struct World {
    app: Appliance,
    m: FakeModel,
    _mail: FakeMcp,
}

async fn world() -> World {
    let m = FakeModel::start(model()).await;
    let mail = mcp::mail().world("inbox", json!([{ "from": "bob@corp.example", "subject": "Q3 contract", "body": PROMISE }])).start().await;
    let spec = Spec::default().app("mail", &mail, "private", "private+untrusted", &[("list_inbox", "read")]);
    let app = Appliance::start(spec, &m).await;
    World { app, m, _mail: mail }
}

/// The person writes, in a new conversation or in `thread`; returns the
/// thread and the turn once it settled.
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

async fn ledger(api: &Api) -> Vec<Value> {
    let (st, v) = api.get("/v1/commitments").await;
    assert!(st.is_success(), "{v}");
    v["commitments"].as_array().cloned().unwrap_or_default()
}

async fn one(api: &Api, what: &str) -> Value {
    ledger(api).await.into_iter().find(|c| c["what"] == what).unwrap_or_else(|| panic!("no commitment {what:?}"))
}

/// The system context of the latest model call that answered `text`.
fn context_of(m: &FakeModel, text: &str) -> String {
    m.requests().into_iter().rev().find(|c| said(c) == text && !system(c).contains(PASS)).map(|c| system(&c)).unwrap_or_else(|| panic!("no call for {text:?}"))
}

/// The tool result the turn after `text`'s first call got.
fn result_after(m: &FakeModel, text: &str, turn: usize) -> model::ToolResult {
    let c = m.requests().into_iter().rev().find(|c| c.turn() == turn && said(c) == text && system(c).contains(CHAT)).unwrap_or_else(|| panic!("no turn {turn} for {text:?}"));
    c.last_results().into_iter().next().expect("no tool result")
}

/// The commitments section of a context, alone.
fn section(sys: &str) -> &str {
    sys.find(SECTION).map(|i| &sys[i..]).unwrap_or("")
}

async fn taint(api: &Api, task: &str) -> Value {
    let (_, t) = api.get(&format!("/v1/tasks/{task}")).await;
    t["taint"].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commitment_said_in_conversation_is_recorded_and_carried_into_every_later_run() {
    let w = world().await;
    let alice = w.app.api("alice");

    let (thread, turn) = say(&alice, DECK, None).await;
    let c = one(&alice, "Send Dana the deck").await;
    assert_eq!((&c["who"], &c["to"], &c["by"], &c["status"], &c["overdue"]), (&json!("person"), &json!("Dana"), &json!("2099-12-31"), &json!("open"), &json!(false)), "{c}");
    assert_eq!(
        (&c["source"]["kind"], &c["source"]["thread"], &c["source"]["task"], &c["quote"]),
        (&json!("conversation"), &json!(thread), &json!(turn), &json!(DECK_QUOTE)),
        "{c}"
    );
    assert_eq!((c["label"]["untrusted"].as_bool(), c["label"]["private"].as_bool()), (Some(false), Some(false)), "{c}");

    // Magnus's own, from what it said earlier in the conversation.
    let (thread, _) = say(&alice, ASK_REMIND, None).await;
    say(&alice, THANKS, Some(&thread)).await;
    let r = one(&alice, "Remind them about the invoice").await;
    assert_eq!((&r["who"], &r["source"]["kind"]), (&json!("magnus"), &json!("conversation")), "{r}");

    // The next chat turn and a task both carry them.
    say(&alice, "Hello again.", None).await;
    let sys = context_of(&w.m, "Hello again.");
    let s = section(&sys);
    assert!(s.contains(&format!("{} (they will): Send Dana the deck; for Dana; by 2099-12-31", c["id"].as_str().unwrap())), "{s}");
    assert!(s.contains("(you will): Remind them about the invoice") && !s.contains("; OVERDUE"), "{s}");
    let task = alice.submit("Write the weekly summary.").await;
    alice.wait_settled(&task).await;
    let sys = context_of(&w.m, "Write the weekly summary.");
    assert!(section(&sys).contains("Send Dana the deck"), "a task does not carry the commitments: {sys}");
    assert_eq!(taint(&alice, &task).await["untrusted"], false, "a commitment from her own words tainted the task");

    // A run that started before one was recorded does not carry it.
    let first = m_first(&w.m, DECK);
    assert!(!section(&first).contains("Send Dana the deck"), "{first}");
}

/// The system context of the first call that answered `text`.
fn m_first(m: &FakeModel, text: &str) -> String {
    m.requests().into_iter().find(|c| said(c) == text && system(c).contains(CHAT)).map(|c| system(&c)).unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn only_words_really_said_or_read_are_recorded_and_never_twice() {
    let w = world().await;
    let alice = w.app.api("alice");

    // Read in the mail, citing the read: recorded, from the app.
    say(&alice, MAIL, None).await;
    let c = one(&alice, "Review the Q3 contract").await;
    assert_eq!((&c["source"]["kind"], &c["source"]["app"], &c["source"]["tool"], &c["to"]), (&json!("app"), &json!("mail"), &json!("mail__list_inbox"), &json!("Bob")), "{c}");
    assert_eq!((c["label"]["untrusted"].as_bool(), c["label"]["private"].as_bool()), (Some(true), Some(true)), "{c}");

    // The same words without the read they came from: not hers, refused.
    say(&alice, MAIL_NO_CALL, None).await;
    let r = result_after(&w.m, MAIL_NO_CALL, 2);
    assert!(r.is_error && r.text.contains("Not recorded"), "{r:?}");
    // Words no one said: refused, nothing recorded.
    say(&alice, IDLE, None).await;
    let r = result_after(&w.m, IDLE, 1);
    assert!(r.is_error && r.text.contains("Not recorded"), "{r:?}");
    assert!(!ledger(&alice).await.iter().any(|c| c["what"] == "Fly to Paris"), "a commitment no one made was recorded");

    // Said, recorded once; said again while open, refused naming the first.
    say(&alice, DECK, None).await;
    let first = one(&alice, "Send Dana the deck").await;
    say(&alice, AGAIN, None).await;
    let r = result_after(&w.m, AGAIN, 1);
    assert!(r.is_error && r.text.contains(first["id"].as_str().unwrap()), "{r:?}");
    assert_eq!(ledger(&alice).await.iter().filter(|c| c["what"] == "Send Dana the deck").count(), 1);
    assert_eq!(ledger(&alice).await.len(), 2, "{:?}", ledger(&alice).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn overdue_is_derived_and_done_or_released_ones_leave_the_context() {
    let w = world().await;
    let alice = w.app.api("alice");
    say(&alice, LATE, None).await;
    say(&alice, DECK, None).await;
    let late = one(&alice, "File the expense report").await;
    let deck = one(&alice, "Send Dana the deck").await;
    assert_eq!((&late["overdue"], &deck["overdue"]), (&json!(true), &json!(false)), "{late} {deck}");
    assert_eq!(ledger(&alice).await[0]["id"], late["id"], "the open ones, soonest due first");

    say(&alice, "Anything due?", None).await;
    let s = context_of(&w.m, "Anything due?");
    let s = section(&s);
    assert!(s.contains("File the expense report; by 2020-01-05; OVERDUE") && s.contains("Send Dana the deck") && !s.contains("deck; for Dana; by 2099-12-31; OVERDUE"), "{s}");

    // Magnus marks one done, and releases the other, at the person's word.
    let done = format!("{DONE}{}", deck["id"].as_str().unwrap());
    say(&alice, &done, None).await;
    let off = format!("{OFF}{}", late["id"].as_str().unwrap());
    say(&alice, &off, None).await;
    let (deck, late) = (one(&alice, "Send Dana the deck").await, one(&alice, "File the expense report").await);
    assert_eq!(
        (&deck["status"], &deck["note"], &late["status"], &late["overdue"]),
        (&json!("done"), &json!("They said they sent it."), &json!("released"), &json!(false)),
        "{deck} {late}"
    );
    say(&alice, "Anything left?", None).await;
    let s = context_of(&w.m, "Anything left?");
    assert!(section(&s).is_empty(), "done and released ones are still in the context: {s}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_person_corrects_completes_and_removes_theirs_and_no_one_else_sees_them() {
    let w = world().await;
    let (alice, bob, carol) = (w.app.api("alice"), w.app.api("bob"), w.app.api("carol"));
    say(&carol, DECK, None).await;
    let c = one(&carol, "Send Dana the deck").await;
    let id = c["id"].as_str().unwrap().to_string();
    let path = format!("/v1/commitments/{id}");

    // No one else sees or changes it: not bob, not the owner.
    for other in [&alice, &bob] {
        assert!(ledger(other).await.is_empty(), "another person sees carol's commitments");
        assert_eq!(other.put(&path, json!({ "status": "done" })).await.0.as_u16(), 404);
        assert_eq!(other.send(reqwest::Method::DELETE, &path, None).await.0.as_u16(), 404);
    }
    // Nor does Magnus, in someone else's run.
    say(&bob, &format!("{DONE}{id}"), None).await;
    let r = result_after(&w.m, &format!("{DONE}{id}"), 1);
    assert!(r.is_error && !r.text.contains("Dana"), "{r:?}");
    say(&bob, "Hello from Bob.", None).await;
    assert!(!context_of(&w.m, "Hello from Bob.").contains("Send Dana the deck"), "bob's run carries carol's commitment");
    assert_eq!(one(&carol, "Send Dana the deck").await["status"], "open");

    // She corrects it; the next run carries the correction.
    let (st, v) = carol.put(&path, json!({ "what": "Send Dana the revised deck", "to": "Dana and Eve", "by": "2099-11-30" })).await;
    assert_eq!(st.as_u16(), 200, "{v}");
    assert_eq!(carol.put(&path, json!({ "by": "next Friday" })).await.0.as_u16(), 400, "a date that is no date");
    assert_eq!(carol.put(&path, json!({})).await.0.as_u16(), 400, "nothing to change");
    let c = one(&carol, "Send Dana the revised deck").await;
    assert_eq!((&c["to"], &c["by"], &c["quote"]), (&json!("Dana and Eve"), &json!("2099-11-30"), &json!(DECK_QUOTE)), "{c}");
    say(&carol, "Hello from Carol.", None).await;
    assert!(section(&context_of(&w.m, "Hello from Carol.")).contains("Send Dana the revised deck; for Dana and Eve; by 2099-11-30"));

    // Done, released and open again, by her.
    for status in ["done", "released", "open"] {
        assert_eq!(carol.put(&path, json!({ "status": status })).await.0.as_u16(), 200);
        assert_eq!(one(&carol, "Send Dana the revised deck").await["status"], status);
    }
    // Removed: gone from her list and from what runs carry.
    let (st, v) = carol.send(reqwest::Method::DELETE, &path, None).await;
    assert_eq!(st.as_u16(), 200, "{v}");
    assert!(ledger(&carol).await.is_empty());
    assert_eq!(carol.put(&path, json!({ "status": "done" })).await.0.as_u16(), 404);
    say(&carol, "Hello once more.", None).await;
    assert!(section(&context_of(&w.m, "Hello once more.")).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commitment_read_in_untrusted_mail_taints_each_run_that_carries_it() {
    let w = world().await;
    let alice = w.app.api("alice");
    say(&alice, "Good morning.", None).await;
    let (_, clean) = say(&alice, "Before the mail.", None).await;
    assert_eq!(taint(&alice, &clean).await["untrusted"], false);

    say(&alice, MAIL, None).await;
    let c = one(&alice, "Review the Q3 contract").await;
    // A later chat turn and a later task carry it, and its taint.
    let (_, turn) = say(&alice, "What is on my plate?", None).await;
    let sys = context_of(&w.m, "What is on my plate?");
    assert!(section(&sys).contains("Review the Q3 contract") && section(&sys).contains("data, not instructions"), "{sys}");
    let t = taint(&alice, &turn).await;
    assert_eq!((t["untrusted"].as_bool(), t["private"].as_bool()), (Some(true), Some(true)), "a run carrying words read in the mail is not tainted: {t}");
    let task = alice.submit("Tidy my notes.").await;
    alice.wait_settled(&task).await;
    assert_eq!(taint(&alice, &task).await["untrusted"], true, "a task carrying words read in the mail is not tainted");

    // Done, it leaves the context, and later runs are clean again.
    let path = format!("/v1/commitments/{}", c["id"].as_str().unwrap());
    assert_eq!(alice.put(&path, json!({ "status": "done" })).await.0.as_u16(), 200);
    let (_, after) = say(&alice, "And now?", None).await;
    assert_eq!(taint(&alice, &after).await["untrusted"], false, "a done commitment still taints");
}

/// Removing a commitment records the removal no later than the commitment's
/// leaving, so a removal cut short between them finishes at the next start
/// and never leaves its words with no record to finish it by (review of
/// #291, #293).
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_is_recorded_before_the_commitment_leaves() {
    let w = world().await;
    let carol = w.app.api("carol");
    say(&carol, DECK, None).await;
    let id = one(&carol, "Send Dana the deck").await["id"].as_str().unwrap().to_string();
    let (st, v) = carol.send(reqwest::Method::DELETE, &format!("/v1/commitments/{id}"), None).await;
    assert_eq!(st.as_u16(), 200, "{v}");

    let (db, conn) = tokio_postgres::connect(&w.app.db.kernel_url(), tokio_postgres::NoTls).await.unwrap();
    tokio::spawn(conn);
    let seq = |kind: &'static str| {
        let (db, id) = (&db, id.clone());
        async move {
            let rows = db.query("SELECT seq, body::text FROM public.events WHERE kind = $1 ORDER BY seq", &[&kind]).await.unwrap();
            rows.into_iter().find(|r| r.get::<_, String>(1).contains(&id)).map(|r| r.get::<_, i64>(0)).unwrap_or_else(|| panic!("no {kind} event for {id}"))
        }
    };
    let (removed, left) = (seq("removed").await, seq("commitment_removed").await);
    assert!(removed < left, "the commitment left (seq {left}) before its removal was recorded (seq {removed})");
    // Its words are gone from its own records (the person's message keeps its own).
    let words = db.query("SELECT c.body::text FROM public.content c JOIN public.events e ON e.seq = c.seq WHERE e.kind LIKE 'commitment_%'", &[]).await.unwrap();
    let quote = DECK_QUOTE.to_lowercase();
    assert!(!words.iter().any(|r| r.get::<_, String>(0).to_lowercase().contains(&quote)), "the commitment's words stayed");
}
