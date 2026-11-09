//! Composed answers in chat (U8), through the HTTP API: the answer's text
//! first, then only components that carry what the text does not, from the
//! fixed library and checked by the kernel; nothing for a plain question;
//! and read aloud from a script written for listening, not the screen text.

use magnus_invariants::model::Script;
use magnus_invariants::*;
use magnus_scratch::say;
use serde_json::{json, Value};

const CHAT: &str = "talking with the person you work for in a chat";
const LISTENING: &str = "to be heard rather than read";

const DATA_QUESTION: &str = "How did revenue go by quarter?";
const DATA_ANSWER: &str = "Revenue rose every quarter, to 41.7M in Q3.";
const SCRIPT: &str = "Revenue went up every quarter, reaching forty one point seven million in the third.";

fn said(c: &Conversation) -> String {
    let first = c.messages().first().cloned().unwrap_or_default();
    first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string()
}

fn chart() -> Value {
    json!([
        { "type": "bar_chart", "title": "Revenue by quarter", "labels": ["Q1", "Q2", "Q3"], "values": [35.1, 38.2, 41.7], "unit": "M" },
        { "type": "table", "columns": ["Quarter", "Revenue (M)"], "rows": [["Q1", 35.1], ["Q2", 38.2], ["Q3", 41.7]] }
    ])
}

fn offices() -> Value {
    json!([
        { "type": "map", "title": "Offices", "places": [{ "name": "London", "lat": 51.5, "lon": -0.12 }, { "name": "Paris", "lat": 48.86, "lon": 2.35, "note": "opened in May" }] },
        { "type": "timeline", "events": [{ "when": "2023", "label": "London opens" }, { "when": "May 2025", "label": "Paris opens" }] },
        { "type": "comparison", "options": ["London", "Paris"], "aspects": [{ "name": "Staff", "values": [40, 12], "best": 0 }, { "name": "Rent", "values": ["high", "medium"], "best": 1 }] },
        { "type": "code", "language": "sql", "code": "select city, staff from offices;" }
    ])
}

/// The data question: answer text and its chart in one reply. The places
/// question: a paragraph is refused, a bad coordinate is refused, then the
/// map, timeline, comparison and code are shown. Anything else: text only.
fn model() -> Script {
    script(|c| async move {
        let system = c.body["system"].to_string();
        if system.contains(LISTENING) {
            return Reply::Text(SCRIPT.into());
        }
        if !system.contains(CHAT) {
            return match c.turn() {
                0 => tool("show", json!({ "components": chart() })),
                _ => tool("task_fail", json!({ "reason": "nothing to show here" })),
            };
        }
        let s = said(&c);
        match (s.as_str(), c.turn()) {
            (DATA_QUESTION, 0) => tools(vec![say(DATA_ANSWER), ("show", json!({ "components": chart() }))]),
            ("Where are our offices?", 0) => tool("show", json!({ "components": [{ "type": "text", "text": "We have two offices." }] })),
            ("Where are our offices?", 1) => tool("show", json!({ "components": [{ "type": "map", "places": [{ "name": "Nowhere", "lat": 200, "lon": 0 }] }] })),
            ("Where are our offices?", 2) => tool("show", json!({ "components": offices() })),
            ("Where are our offices?", _) => Reply::Text("Two offices, London and Paris.".into()),
            _ => Reply::Text(format!("Answer to: {s}")),
        }
    })
}

async fn start() -> (FakeModel, Appliance) {
    let mut m = FakeModel::start(model()).await;
    m.url = magnus_scratch::provider(&m).await;
    let app = Appliance::start(Spec::default(), &m).await;
    (m, app)
}

async fn ask(api: &Api, text: &str, thread: Option<&str>) -> (String, Value) {
    let mut body = json!({ "text": text });
    if let Some(t) = thread {
        body["thread"] = json!(t);
    }
    let (st, v) = api.post("/v1/chat", body).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let thread = v["thread"].as_str().unwrap().to_string();
    api.wait_settled(v["task_id"].as_str().unwrap()).await;
    let (_, t) = api.get(&format!("/v1/chat/{thread}")).await;
    let answer = t["entries"].as_array().unwrap().iter().rev().find(|e| e["kind"] == "answer").cloned().unwrap_or_else(|| panic!("no answer: {t}"));
    (thread, answer)
}

fn chat_calls(m: &FakeModel, text: &str) -> Vec<Conversation> {
    m.requests().into_iter().filter(|c| c.body["system"].to_string().contains(CHAT) && said(c) == text).collect()
}

/// A data question gets its text and then a chart and a table, in the same
/// reply: the turn ends there, without another model call. A plain question
/// gets text only.
#[tokio::test(flavor = "multi_thread")]
async fn a_data_question_gets_text_then_a_chart_and_a_plain_one_text_only() {
    let (m, app) = start().await;
    let alice = app.api("alice");
    let (thread, a) = ask(&alice, DATA_QUESTION, None).await;
    assert_eq!((a["text"].clone(), a["view"].clone()), (json!(DATA_ANSWER), chart()), "{a}");
    assert_eq!(chat_calls(&m, DATA_QUESTION).len(), 1, "the turn asked the model again after its answer and chart");
    assert!(chat_calls(&m, DATA_QUESTION)[0].offered_tools().contains(&"show".to_string()));

    let (_, a) = ask(&alice, "Thanks, and who are you?", Some(&thread)).await;
    assert_eq!(a["text"], "Answer to: Thanks, and who are you?");
    assert!(a["view"].is_null(), "a plain question got components: {a}");
    // What was shown is part of what the next turn reads.
    let heard = chat_calls(&m, "Thanks, and who are you?")[0].messages()[0].to_string();
    assert!(heard.contains("Shown below the answer") && heard.contains("Revenue by quarter"), "{heard}");
}

/// Chat shows only what the text cannot carry, from the fixed library, each
/// checked; what is refused says why, and the turn goes on.
#[tokio::test(flavor = "multi_thread")]
async fn shown_components_are_checked_and_prose_is_refused() {
    let (m, app) = start().await;
    let (_, a) = ask(&app.api("alice"), "Where are our offices?", None).await;
    assert_eq!((a["text"].clone(), a["view"].clone()), (json!("Two offices, London and Paris."), offices()), "{a}");
    let results: Vec<String> = chat_calls(&m, "Where are our offices?").iter().skip(1).flat_map(|c| c.last_results()).map(|r| format!("{}: {}", r.is_error, r.text)).collect();
    assert!(results[0].starts_with("true: ") && results[0].contains("say that in your answer"), "{results:?}");
    assert!(results[1].starts_with("true: ") && results[1].contains("lat must be a number from -90 to 90"), "{results:?}");
    assert!(results[2].starts_with("false: "), "{results:?}");
}

/// A task's outcome is its deliverable; show is for chat.
#[tokio::test(flavor = "multi_thread")]
async fn a_task_cannot_show() {
    let (m, app) = start().await;
    let alice = app.api("alice");
    let (_, v) = alice.post("/v1/tasks", json!({ "text": "Chart the revenue" })).await;
    let t = alice.wait_settled(v["task_id"].as_str().unwrap()).await;
    assert_eq!(t["status"], "failed", "{t}");
    let r = m.at_turn(1).unwrap().last_results();
    assert!(r[0].is_error && r[0].text.contains("show is for chat"), "{r:?}");
}

/// Read aloud uses a script written for listening by a call with no tools,
/// once; the screen text is not read out.
#[tokio::test(flavor = "multi_thread")]
async fn read_aloud_uses_a_script_written_for_listening() {
    let (m, app) = start().await;
    let alice = app.api("alice");
    let (thread, a) = ask(&alice, DATA_QUESTION, None).await;
    let path = format!("/v1/chat/{thread}/entries/{}/spoken", a["id"].as_str().unwrap());
    let (st, v) = alice.post(&path, json!({})).await;
    assert_eq!((st.as_u16(), v["script"].clone()), (200, json!(SCRIPT)), "{v}");
    let calls: Vec<Conversation> = m.requests().into_iter().filter(|c| c.body["system"].to_string().contains(LISTENING)).collect();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].offered_tools().is_empty(), "the script writer was offered tools");
    let read = calls[0].messages()[0].to_string();
    assert!(read.contains(DATA_ANSWER) && read.contains("Revenue by quarter"), "the script writer did not read the answer and its chart: {read}");

    assert_eq!(alice.post(&path, json!({})).await.1["script"], SCRIPT);
    assert_eq!(m.requests().into_iter().filter(|c| c.body["system"].to_string().contains(LISTENING)).count(), 1, "the script was written twice");
    assert_eq!(app.api("carol").post(&path, json!({})).await.0, 404, "someone else had alice's answer read");
    let mine = format!("/v1/chat/{thread}/entries/m:{}/spoken", a["task_id"].as_str().unwrap());
    assert_eq!(alice.post(&mine, json!({})).await.0, 400, "the person's own message was rewritten");
    let (_, again) = alice.get(&format!("/v1/chat/{thread}")).await;
    assert_eq!(again["entries"].as_array().unwrap().iter().find(|e| e["kind"] == "answer").unwrap()["spoken"], SCRIPT);
}

/// Read aloud at the run's model-call cap reads the answer itself: no
/// script is written past the cap (#330).
#[tokio::test(flavor = "multi_thread")]
async fn read_aloud_at_the_call_cap_reads_the_answer_itself() {
    let mut m = FakeModel::start(model()).await;
    m.url = magnus_scratch::provider(&m).await;
    let app = Appliance::start(Spec::default().max_model_calls(1), &m).await;
    let alice = app.api("alice");
    let (thread, a) = ask(&alice, "Who are you?", None).await;
    let path = format!("/v1/chat/{thread}/entries/{}/spoken", a["id"].as_str().unwrap());
    let (st, v) = alice.post(&path, json!({})).await;
    assert_eq!(st.as_u16(), 200, "{v}");
    let calls: Vec<Conversation> = m.requests().into_iter().filter(|c| c.body["system"].to_string().contains(LISTENING)).collect();
    assert!(calls.is_empty(), "a script was written past the call cap: {v}");
    assert_eq!(v["script"], a["text"], "read aloud did not fall back to the answer: {v}");
}

/// The answer's text streams before anything is shown: someone who opens the
/// chat while the model is still writing gets the text so far at once, and
/// the chart only when the answer is done.
#[tokio::test(flavor = "multi_thread")]
async fn the_text_streams_first_and_a_late_viewer_gets_it_so_far() {
    let (_m, app) = start().await;
    let alice = app.api("alice");
    let (_, v) = alice.post("/v1/chat", json!({ "text": DATA_QUESTION })).await;
    let (thread, task) = (v["thread"].as_str().unwrap().to_string(), v["task_id"].as_str().unwrap().to_string());
    // Join after the text was written, while the tool calls are still to come.
    tokio::time::sleep(magnus_scratch::SAID_PAUSE / 2).await;
    let mut live = reqwest::Client::new().get(format!("{}/v1/tasks/{task}/live", alice.base)).bearer_auth(&alice.token).send().await.unwrap();
    let first = tokio::time::timeout(std::time::Duration::from_secs(1), live.chunk()).await.expect("nothing streamed to a late viewer").unwrap().unwrap();
    assert!(String::from_utf8_lossy(&first).contains(DATA_ANSWER), "{first:?}");
    let (_, t) = alice.get(&format!("/v1/chat/{thread}")).await;
    assert!(!t["entries"].as_array().unwrap().iter().any(|e| e["view"].is_array()), "the chart came before the text was done: {t}");
    alice.wait_settled(&task).await;
    let (_, t) = alice.get(&format!("/v1/chat/{thread}")).await;
    assert!(t["entries"].as_array().unwrap().iter().any(|e| e["view"] == chart()), "{t}");
}
