//! Auxiliary model calls are worst-case priced against what is left: the
//! verifier, the memory pass, knowledge extraction and spoken scripts run
//! whole or not at all, and never suspend the run for going without (#319).

use magnus_invariants::appliance::bin;
use magnus_invariants::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// One loop turn's reply: its tool calls, or its text, and whether it
/// spends the budget.
struct Turn {
    calls: Vec<(String, Value)>,
    text: Option<String>,
    spend: bool,
}

fn plain(calls: Vec<(String, Value)>) -> Turn {
    Turn { calls, text: None, spend: false }
}

fn spending(calls: Vec<(String, Value)>) -> Turn {
    Turn { calls, text: None, spend: true }
}

fn text_spending(text: &str) -> Turn {
    Turn { calls: vec![], text: Some(text.into()), spend: true }
}

fn pair(r: Reply) -> (String, Value) {
    match r {
        Reply::Tools(v) => v.into_iter().next().unwrap(),
        _ => unreachable!(),
    }
}

/// A chat-completions provider that bills what the test says. Loop turns
/// follow the test's script, keyed by the task's text and the turn; a
/// spending turn reports 450k input tokens, about $0.90 of the $1 budget:
/// model actions are still allowed below the dollar limit, but no
/// auxiliary call's worst case fits what is left. Auxiliary calls are
/// answered so the behavior shows: a verdict that holds, a filed fact, a
/// bare number, a polished script, silence for a criteria review.
struct BillableChat {
    url: String,
    bodies: Arc<Mutex<Vec<Value>>>,
}

type Script = Arc<dyn Fn(String, usize) -> Turn + Send + Sync>;

impl BillableChat {
    async fn start(script: Script) -> Self {
        use axum::{
            extract::State,
            routing::{get, post},
            Json, Router,
        };
        struct S {
            script: Script,
            bodies: Arc<Mutex<Vec<Value>>>,
            next_id: std::sync::atomic::AtomicUsize,
        }
        let s = Arc::new(S { script, bodies: Arc::new(Mutex::new(vec![])), next_id: std::sync::atomic::AtomicUsize::new(0) });
        let app = Router::new()
            .route("/models", get(|| async { Json(json!({ "data": [{ "id": "glm-test" }] })) }))
            .route(
                "/chat/completions",
                post(|State(s): State<Arc<S>>, Json(b): Json<Value>| async move {
                    s.bodies.lock().unwrap().push(b.clone());
                    let system = b["messages"].as_array().and_then(|m| m.first()).and_then(|m| m["content"].as_str()).unwrap_or_default().to_string();
                    // Auxiliary calls first: their system prompts say what
                    // they are (the loop offers memory_write too, so the
                    // tools do not tell them apart).
                    enum Shape {
                        Tools(Vec<(String, Value)>),
                        Text(String),
                    }
                    let (shape, spend) = if system.contains("You verify a task's work") {
                        (Shape::Tools(vec![("verdict".into(), json!({ "criteria": [{ "id": "c1", "holds": true, "evidence": "read it in the world" }] }))]), false)
                    } else if system.contains("You keep the person's long-term memory") {
                        (Shape::Tools(vec![("memory_write".into(), json!({ "fact": "The person keeps a labrador." }))]), false)
                    } else if system.contains("Read one value") {
                        (Shape::Text("41.7".into()), false)
                    } else if system.contains("review a task's acceptance criteria") {
                        (Shape::Text("none".into()), false)
                    } else if system.contains("to be heard rather than read") {
                        (Shape::Text("Revenue went up every quarter, reaching forty one point seven million in the third.".into()), false)
                    } else {
                        let msgs = b["messages"].as_array().cloned().unwrap_or_default();
                        let text = msgs
                            .iter()
                            .find(|m| m["role"] == "user")
                            .and_then(|m| m["content"].as_array())
                            .and_then(|p| p.iter().find_map(|x| x["text"].as_str()))
                            .unwrap_or_default()
                            .to_string();
                        let turn = msgs.iter().filter(|m| m["role"] == "assistant").count();
                        let t = (s.script)(text, turn);
                        match t.text {
                            Some(text) => (Shape::Text(text), t.spend),
                            None => (Shape::Tools(t.calls), t.spend),
                        }
                    };
                    let (delta, finish) = match shape {
                        Shape::Tools(calls) => (
                            json!({ "tool_calls": calls.into_iter().enumerate().map(|(i, (name, input))| json!({ "index": i, "id": format!("call_{}", s.next_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst)), "function": { "name": name, "arguments": input.to_string() } })).collect::<Vec<_>>() }),
                            "tool_calls",
                        ),
                        Shape::Text(t) => (json!({ "content": t }), "stop"),
                    };
                    let frame = json!({ "model": "glm-test", "choices": [{ "delta": delta, "finish_reason": finish }] });
                    let input = if spend { 450_000 } else { 100 };
                    let usage = json!({ "usage": { "prompt_tokens": input, "completion_tokens": 20 } });
                    let body = format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n", frame, usage);
                    ([("content-type", "text/event-stream")], body)
                }),
            )
            .with_state(s.clone());
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        BillableChat { url, bodies: s.bodies.clone() }
    }

    fn requests(&self) -> Vec<Value> {
        self.bodies.lock().unwrap().clone()
    }

    /// What the model was asked with this in its system prompt.
    fn asked_with(&self, needle: &str) -> Vec<Value> {
        self.requests().into_iter().filter(|b| b["messages"][0]["content"].as_str().is_some_and(|s| s.contains(needle))).collect()
    }
}

/// The billable adapter on the appliance: chat API, $2/$10 test prices.
async fn add_billable(app: &mut Appliance, chat: &BillableChat) {
    let alice = app.api("alice");
    assert!(alice.put_secret("oa_credential", "test-key").await.is_success());
    let path = app.dir.path().join("magnus.toml");
    let mut cfg: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    let oa = json!({
        "id": "oa",
        "command": [bin("magnus-adapter-openai")],
        "settings": { "api": "chat", "base_url": chat.url, "credential": "oa_credential",
            "prices": { "glm": { "input": 2.0, "output": 10.0, "cache_read": 0.2 } } }
    });
    cfg["adapters"].as_array_mut().unwrap().push(toml::Value::try_from(oa).unwrap());
    std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
    app.restart().await;
}

/// A task whose completion spends its last dollar: the verifier has nothing
/// to judge with, so the criterion stays unconfirmed, with no call (#319).
#[tokio::test(flavor = "multi_thread")]
async fn a_verifier_with_no_dollars_left_judges_nothing() {
    let chat = BillableChat::start(Arc::new(|_, turn| match turn {
        0 => plain(vec![pair(contract_unchecked("do the work", "the work is done"))]),
        _ => spending(vec![pair(complete("done"))]),
    }))
    .await;
    let m = FakeModel::start(script(|_| async { Reply::Text("unused".into()) })).await;
    let mut app = Appliance::start(Spec { max_usd: 1.0, verifier: true, ..Spec::default() }, &m).await;
    add_billable(&mut app, &chat).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/tasks", json!({ "text": "Do the work.", "model": { "adapter": "oa", "model": "glm-test" } })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let t = alice.wait_settled(v["task_id"].as_str().unwrap()).await;
    assert!(chat.asked_with("You verify a task's work").is_empty(), "the verifier was asked with no dollars left: {t}");
    assert_eq!(t["status"], "completed", "the unverified criterion did not stay unconfirmed: {t}");
}

/// A run that ends with its dollars spent files no memory: the pass is
/// skipped, and the person's memories stay as they were (#319).
#[tokio::test(flavor = "multi_thread")]
async fn a_memory_pass_with_no_dollars_left_files_nothing() {
    // A checked criterion: the code check passes, the verifier stays out of
    // it either way, and only the memory pass differs.
    let chat = BillableChat::start(Arc::new(|_, turn| match turn {
        0 => plain(vec![pair(contract_checked("do the work", "knowledge_search", json!({ "query": "zzz-nothing" }), "/entries", json!([])))]),
        _ => spending(vec![pair(complete("done"))]),
    }))
    .await;
    let m = FakeModel::start(script(|_| async { Reply::Text("unused".into()) })).await;
    let mut app = Appliance::start(Spec { max_usd: 1.0, ..Spec::default() }, &m).await;
    add_billable(&mut app, &chat).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/tasks", json!({ "text": "Do the work.", "model": { "adapter": "oa", "model": "glm-test" } })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let t = alice.wait_settled(v["task_id"].as_str().unwrap()).await;
    assert_eq!(t["status"].as_str(), Some("completed"), "the run did not complete: {t}");
    assert!(chat.asked_with("You keep the person's long-term memory").is_empty(), "the memory pass was asked with no dollars left: {t}");
    let (_, mem) = alice.get("/v1/memory").await;
    assert_eq!(mem["memory"].as_array().map(Vec::len), Some(0), "a memory was filed with no dollars left: {mem}");
}

/// A typed value whose reading would pass the budget is not read: the first
/// filing (funded) becomes a company fact, the second (spent) does not (#319).
#[tokio::test(flavor = "multi_thread")]
async fn an_extraction_with_no_dollars_left_reads_nothing() {
    let web = FakeWeb::start(&[("/a", "<p>Quarterly figure: 41.7.</p>")]).await;
    let url = web.url("/a");
    let chat = BillableChat::start(Arc::new(move |_, turn| match turn {
        0 => plain(vec![pair(contract_unchecked("file the figures", "the figures are filed"))]),
        1 => plain(vec![("web_fetch".into(), json!({ "url": url.clone() }))]),
        2 => plain(vec![(
            "knowledge_add".into(),
            json!({ "title": "Page A", "text": "The page reports 41.7.",
            "citations": [{ "source": url.clone(), "quote": "41.7" }], "value": { "key": "figure_a", "type": "number" } }),
        )]),
        _ => spending(vec![(
            "knowledge_add".into(),
            json!({ "title": "Page B", "text": "The page reports 41.7.",
            "citations": [{ "source": url.clone(), "quote": "41.7" }], "value": { "key": "figure_b", "type": "number" } }),
        )]),
    }))
    .await;
    let m = FakeModel::start(script(|_| async { Reply::Text("unused".into()) })).await;
    let mut app = Appliance::start(Spec { max_usd: 1.0, ..Spec::default() }, &m).await;
    add_billable(&mut app, &chat).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/tasks", json!({ "text": "File the figures.", "model": { "adapter": "oa", "model": "glm-test" } })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let t = alice.wait_settled(v["task_id"].as_str().unwrap()).await;
    assert_eq!(chat.asked_with("Read one value").len(), 1, "the spent extraction went out: {t}");
    let (_, found) = alice.get("/v1/knowledge?q=figure").await;
    let facts: Vec<String> = found["entries"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e["integrity"] == "company")
        .filter_map(|e| e["value"]["key"].as_str().map(String::from))
        .collect();
    assert_eq!(facts, vec!["figure_a".to_string()], "the spent extraction filed a fact: {found}");
}

/// Read aloud with no dollars left reads the answer itself: no script is
/// written, and the screen text is what is heard (#319).
#[tokio::test(flavor = "multi_thread")]
async fn read_aloud_with_no_dollars_left_reads_the_answer_itself() {
    // One text turn: the answer, spending all but the auxiliary change.
    let chat = BillableChat::start(Arc::new(|_, _| text_spending("Revenue rose every quarter."))).await;
    let m = FakeModel::start(script(|_| async { Reply::Text("unused".into()) })).await;
    let mut app = Appliance::start(Spec { max_usd: 1.0, ..Spec::default() }, &m).await;
    add_billable(&mut app, &chat).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/chat", json!({ "text": "How did revenue go?", "model": { "adapter": "oa", "model": "glm-test" } })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let thread = v["thread"].as_str().unwrap().to_string();
    alice.wait_settled(v["task_id"].as_str().unwrap()).await;
    let (_, t) = alice.get(&format!("/v1/chat/{thread}")).await;
    let a = t["entries"].as_array().unwrap().iter().rev().find(|e| e["kind"] == "answer").cloned().unwrap_or_else(|| panic!("no answer: {t}"));
    let (st, v) = alice.post(&format!("/v1/chat/{thread}/entries/{}/spoken", a["id"].as_str().unwrap()), json!({})).await;
    assert_eq!(st.as_u16(), 200, "{v}");
    assert!(chat.asked_with("to be heard rather than read").is_empty(), "a script was written with no dollars left: {v}");
    assert_eq!(v["script"], "Revenue rose every quarter.", "read aloud did not fall back to the answer: {v}");
}
