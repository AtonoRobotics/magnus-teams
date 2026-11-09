//! A model call is paid for, failed or not: before each one, a retry too,
//! the kernel reads the run as it is now, and stops without asking when the
//! dollars are spent or the task has been stopped while it waited (#188).
//! It also checks the next call's worst case before asking: a budget one
//! call would exceed stops the run first, and what is left bounds the
//! call's output (#273).

use magnus_invariants::appliance::bin;
use magnus_invariants::*;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A failed call the provider charged for takes the run past its dollar
/// limit: the run stops on its budget, and the provider is not asked again.
#[tokio::test(flavor = "multi_thread")]
async fn a_charged_failure_past_the_dollar_limit_asks_the_model_no_more() {
    // A million input tokens at the test model's $2 a million: $2, past $1.
    let model = FakeModel::start(script(|_| async { Reply::Charged(1_000_000) })).await;
    let app = Appliance::start(Spec { max_usd: 1.0, ..Spec::default() }, &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Say hello.").await;
    let t = alice.wait_settled(&id).await;
    let asked = model.requests().len();
    assert_eq!(asked, 1, "the model was asked {asked} times after a charged failure spent the task's dollars: {t}");
    let stop = alice.inbox().await.into_iter().find(|i| i["task_id"] == id.as_str()).unwrap_or_default();
    assert_eq!(stop["details"]["reason"], "budget", "the run did not stop on its budget: {stop}");
}

/// A task cancelled while the kernel waits to retry an unavailable provider
/// is not retried: the wait ends with nothing more asked.
#[tokio::test(flavor = "multi_thread")]
async fn a_task_cancelled_during_the_retry_wait_is_not_retried() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let model = FakeModel::start(script(move |_| {
        let calls = seen.clone();
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Reply::Status(529)
        }
    }))
    .await;
    let app = Appliance::start(Spec::default().model_retries(2), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Say hello.").await;
    eventually("the first request fails", PATIENCE, || {
        let n = calls.load(Ordering::SeqCst);
        async move { (n >= 1).then_some(()) }
    })
    .await;
    // The first wait is a second; cancel inside it.
    let (st, v) = alice.post(&format!("/v1/tasks/{id}/cancel"), json!({})).await;
    assert!(st.is_success(), "cancel: {st} {v}");
    alice.wait_status(&id, &["cancelled"]).await;
    // Past the longest the first two waits could take.
    tokio::time::sleep(Duration::from_secs(4)).await;
    let asked = model.requests().len();
    assert_eq!(asked, 1, "a cancelled task's model call was retried ({asked} requests)");
}

/// A budget that one call's worst case would exceed stops the run before
/// the first call: the prompt plus the full output, at the model's
/// prices, is checked against what is left, not just what is spent (#273).
#[tokio::test(flavor = "multi_thread")]
async fn a_budget_one_call_would_exceed_asks_the_model_nothing() {
    let model = FakeModel::start(script(|_| async { Reply::Text("Say hello.".into()) })).await;
    let app = Appliance::start(Spec { max_usd: 0.001, ..Spec::default() }, &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Say hello.").await;
    let t = alice.wait_settled(&id).await;
    assert!(model.requests().is_empty(), "the model was asked despite the budget: {t}");
    let stop = alice.inbox().await.into_iter().find(|i| i["task_id"] == id.as_str()).unwrap_or_default();
    assert_eq!(stop["details"]["reason"], "budget", "the run did not stop on its budget: {stop}");
}

/// What is left bounds the call's output: a budget the full output
/// would exceed still lets the call go out, shortened to fit (#273).
#[tokio::test(flavor = "multi_thread")]
async fn what_is_left_bounds_the_call_output() {
    let model = FakeModel::start(script(|c| async move {
        match c.turn() {
            0 => tool(
                "task_contract",
                json!({ "goal": "say hello", "assumptions": [], "irreversible_actions": [],
                    "criteria": [{ "id": "c1", "description": "said", "check": { "tool": "sandbox_exec", "input": { "command": "true" }, "expect": [{ "pointer": "/exit_code", "equals": 0 }] } }] }),
            ),
            _ => tool("task_complete", json!({ "summary": "Said hello.", "deliverable": [{ "type": "text", "text": "hello" }] })),
        }
    }))
    .await;
    let app = Appliance::start(Spec { max_usd: 0.05, ..Spec::default() }, &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Say hello.").await;
    let t = alice.wait_settled(&id).await;
    let asked = model.requests();
    assert!(!asked.is_empty(), "no call went out: {t}");
    let max = asked[0].body["max_tokens"].as_u64().unwrap();
    assert!(max > 0 && max < 16000, "the call went out unclamped at {max}: {t}");
}

/// A chat-completions provider speaking SSE: answers a contract then
/// completions, reports small usage, lists one model, records requests.
struct FakeChat {
    url: String,
    calls: Arc<AtomicUsize>,
    bodies: Arc<Mutex<Vec<Value>>>,
}

impl FakeChat {
    async fn start() -> Self {
        use axum::{
            extract::State,
            routing::{get, post},
            Json, Router,
        };
        struct S {
            calls: Arc<AtomicUsize>,
            bodies: Arc<Mutex<Vec<Value>>>,
        }
        let first = match contract_unchecked("summarise the plan", "summarised") {
            Reply::Tools(v) => v.into_iter().next().unwrap(),
            _ => unreachable!(),
        };
        let rest = match complete("Summarised.") {
            Reply::Tools(v) => v.into_iter().next().unwrap(),
            _ => unreachable!(),
        };
        let s = Arc::new(S { calls: Arc::new(AtomicUsize::new(0)), bodies: Arc::new(Mutex::new(vec![])) });
        let app = Router::new()
            .route("/models", get(|| async { Json(json!({ "data": [{ "id": "glm-test" }] })) }))
            .route(
                "/chat/completions",
                post(|State(s): State<Arc<S>>, Json(b): Json<Value>| async move {
                    let n = s.calls.fetch_add(1, Ordering::SeqCst);
                    s.bodies.lock().unwrap().push(b);
                    let (name, input) = if n == 0 { first.clone() } else { rest.clone() };
                    let frame = json!({
                        "model": "glm-test",
                        "choices": [{ "delta": { "tool_calls": [{ "index": 0, "id": format!("call_{n}"),
                            "function": { "name": name, "arguments": input.to_string() } }] },
                            "finish_reason": "tool_calls" }],
                    });
                    let usage = json!({ "usage": { "prompt_tokens": 100, "completion_tokens": 20 } });
                    let body = format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n", frame, usage);
                    ([("content-type", "text/event-stream")], body)
                }),
            )
            .with_state(s.clone());
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        FakeChat { url, calls: s.calls.clone(), bodies: s.bodies.clone() }
    }

    fn requests(&self) -> Vec<Value> {
        self.bodies.lock().unwrap().clone()
    }
}

/// A large attachment a chat adapter drops from its prompt does not stop
/// a task whose budget covers the real prompt: the quote prices what the
/// adapter sends, not the kernel's bytes (#323).
#[tokio::test(flavor = "multi_thread")]
async fn an_attachment_the_adapter_drops_does_not_stop_a_funded_task() {
    let chat = FakeChat::start().await;
    let m = FakeModel::start(script(|_| async { Reply::Text("unused".into()) })).await;
    let mut app = Appliance::start(Spec { max_usd: 0.5, ..Spec::default() }, &m).await;
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

    // 1.2 MB of PDF: about $0.80 phantom at $2 a million tokens, past the
    // $0.50 budget — but the chat adapter never sends it.
    let pdf = "A".repeat(1_600_000);
    let alice = app.api("alice");
    let (st, v) = alice
        .post(
            "/v1/tasks",
            json!({ "text": "Summarise the attached plan.", "model": { "adapter": "oa", "model": "glm-test" },
                "attachments": [{ "name": "plan.pdf", "media_type": "application/pdf", "data_b64": pdf }] }),
        )
        .await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let id = v["task_id"].as_str().unwrap().to_string();
    let t = alice.wait_settled(&id).await;
    let asked = chat.requests();
    assert!(!asked.is_empty(), "the model was never asked: {t}");
    assert_eq!(asked[0]["max_tokens"], 16000, "the fitting call went out clamped: {t}");
}
