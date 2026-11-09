//! A fake model provider speaking the Anthropic Messages API, driven by a script.
//!
//! The script sees each request and chooses the reply. Turn `n` is the number of
//! assistant messages already in the request, so a run rebuilt from the log after
//! a crash is asked the same turn again. Tool-use ids are random on every reply,
//! so nothing downstream can rely on them being stable.

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use futures::future::BoxFuture;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug)]
pub enum Reply {
    /// One assistant message with these tool calls, in order.
    Tools(Vec<(String, Value)>),
    /// One assistant message with only text and no tool call.
    Text(String),
    /// An HTTP error from the provider.
    Status(u16),
    /// A text reply the provider charged for (this many input tokens) that
    /// names no model: the adapter bills it, then fails it as malformed.
    Charged(u64),
}

pub fn tool(name: &str, input: Value) -> Reply {
    Reply::Tools(vec![(name.to_string(), input)])
}

pub fn tools(calls: Vec<(&str, Value)>) -> Reply {
    Reply::Tools(calls.into_iter().map(|(n, v)| (n.to_string(), v)).collect())
}

#[derive(Clone, Debug)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub text: String,
    pub is_error: bool,
}

#[derive(Clone, Debug)]
pub struct Conversation {
    pub headers: HashMap<String, String>,
    pub body: Value,
}

impl Conversation {
    pub fn turn(&self) -> usize {
        self.messages().iter().filter(|m| m["role"] == "assistant").count()
    }

    pub fn messages(&self) -> Vec<Value> {
        self.body["messages"].as_array().cloned().unwrap_or_default()
    }

    /// The tool results in the final user message.
    pub fn last_results(&self) -> Vec<ToolResult> {
        let msgs = self.messages();
        let Some(last) = msgs.iter().rev().find(|m| m["role"] == "user") else {
            return vec![];
        };
        let Some(blocks) = last["content"].as_array() else {
            return vec![];
        };
        blocks
            .iter()
            .filter(|b| b["type"] == "tool_result")
            .map(|b| ToolResult {
                tool_use_id: b["tool_use_id"].as_str().unwrap_or_default().to_string(),
                text: content_text(&b["content"]),
                is_error: b["is_error"].as_bool().unwrap_or(false),
            })
            .collect()
    }

    /// Everything the model was shown, as one string.
    pub fn text(&self) -> String {
        self.body.to_string()
    }

    pub fn offered_tools(&self) -> Vec<String> {
        self.body["tools"].as_array().map(|a| a.iter().filter_map(|t| t["name"].as_str().map(String::from)).collect()).unwrap_or_default()
    }
}

fn content_text(c: &Value) -> String {
    match c {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        other => other.to_string(),
    }
}

pub type Script = Arc<dyn Fn(Conversation) -> BoxFuture<'static, Reply> + Send + Sync>;

pub fn script<F, Fut>(f: F) -> Script
where
    F: Fn(Conversation) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Reply> + Send + 'static,
{
    Arc::new(move |c| Box::pin(f(c)))
}

/// Reply `replies[turn]`. Past the end, the model gives up with `task_fail`.
pub fn steps(replies: Vec<Reply>) -> Script {
    let replies = Arc::new(replies);
    script(move |c| {
        let replies = replies.clone();
        async move { replies.get(c.turn()).cloned().unwrap_or_else(|| tool("task_fail", json!({ "reason": "script exhausted" }))) }
    })
}

struct Inner {
    script: Mutex<Script>,
    requests: Mutex<Vec<Conversation>>,
    responses: Mutex<Vec<Value>>,
}

#[derive(Clone)]
pub struct FakeModel {
    pub url: String,
    inner: Arc<Inner>,
}

impl FakeModel {
    pub async fn start(s: Script) -> Self {
        let inner = Arc::new(Inner { script: Mutex::new(s), requests: Mutex::new(vec![]), responses: Mutex::new(vec![]) });
        let app = Router::new().route("/v1/messages", post(messages)).with_state(inner.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        FakeModel { url, inner }
    }

    pub fn set_script(&self, s: Script) {
        *self.inner.script.lock().unwrap() = s;
    }

    /// Every request received, in order.
    pub fn requests(&self) -> Vec<Conversation> {
        self.inner.requests.lock().unwrap().clone()
    }

    /// The most recent request asked at turn `n`.
    pub fn at_turn(&self, n: usize) -> Option<Conversation> {
        self.requests().into_iter().rev().find(|c| c.turn() == n)
    }

    /// Every successful response body sent, in order.
    pub fn responses(&self) -> Vec<Value> {
        self.inner.responses.lock().unwrap().clone()
    }

    /// Every tool input the model has ever asked for, by tool name.
    pub fn requested_calls(&self) -> Vec<(String, Value)> {
        self.responses()
            .iter()
            .flat_map(|r| r["content"].as_array().cloned().unwrap_or_default())
            .filter(|b| b["type"] == "tool_use")
            .map(|b| (b["name"].as_str().unwrap_or_default().to_string(), b["input"].clone()))
            .collect()
    }
}

async fn messages(State(inner): State<Arc<Inner>>, headers: HeaderMap, body: String) -> Response {
    let body: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let conv = Conversation { headers: headers.iter().map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or_default().to_string())).collect(), body: body.clone() };
    inner.requests.lock().unwrap().push(conv.clone());
    let s = inner.script.lock().unwrap().clone();
    let reply = s(conv).await;

    let (model, input_tokens) = match reply {
        Reply::Charged(n) => (Value::Null, n),
        _ => (body["model"].clone(), 100),
    };
    let content: Vec<Value> = match reply {
        Reply::Status(code) => {
            let err = json!({"type": "error", "error": {"type": "api_error", "message": "scripted failure"}});
            return (StatusCode::from_u16(code).unwrap(), axum::Json(err)).into_response();
        }
        Reply::Text(t) => vec![json!({"type": "text", "text": t})],
        Reply::Charged(_) => vec![json!({"type": "text", "text": "charged"})],
        Reply::Tools(calls) => calls
            .into_iter()
            .map(|(name, input)| {
                json!({
                    "type": "tool_use",
                    "id": format!("toolu_{}", uuid::Uuid::new_v4().simple()),
                    "name": name,
                    "input": input
                })
            })
            .collect(),
    };
    let stop = if content.iter().any(|b| b["type"] == "tool_use") { "tool_use" } else { "end_turn" };
    let msg = json!({
        "id": format!("msg_{}", uuid::Uuid::new_v4().simple()),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content,
        "stop_reason": stop,
        "stop_sequence": null,
        "usage": {"input_tokens": input_tokens, "output_tokens": 20}
    });
    inner.responses.lock().unwrap().push(msg.clone());

    if body["stream"].as_bool().unwrap_or(false) {
        return sse(&msg);
    }
    axum::Json(msg).into_response()
}

fn sse(msg: &Value) -> Response {
    let mut out = String::new();
    let mut ev = |name: &str, data: Value| out.push_str(&format!("event: {name}\ndata: {data}\n\n"));
    let mut start = msg.clone();
    start["content"] = json!([]);
    start["stop_reason"] = Value::Null;
    ev("message_start", json!({"type": "message_start", "message": start}));
    for (i, b) in msg["content"].as_array().unwrap().iter().enumerate() {
        if b["type"] == "text" {
            ev("content_block_start", json!({"type": "content_block_start", "index": i, "content_block": {"type": "text", "text": ""}}));
            ev("content_block_delta", json!({"type": "content_block_delta", "index": i, "delta": {"type": "text_delta", "text": b["text"]}}));
        } else {
            ev(
                "content_block_start",
                json!({"type": "content_block_start", "index": i,
                "content_block": {"type": "tool_use", "id": b["id"], "name": b["name"], "input": {}}}),
            );
            ev(
                "content_block_delta",
                json!({"type": "content_block_delta", "index": i,
                "delta": {"type": "input_json_delta", "partial_json": b["input"].to_string()}}),
            );
        }
        ev("content_block_stop", json!({"type": "content_block_stop", "index": i}));
    }
    ev("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": msg["stop_reason"], "stop_sequence": null}, "usage": {"output_tokens": 20}}));
    ev("message_stop", json!({"type": "message_stop"}));
    Response::builder().header("content-type", "text/event-stream").body(Body::from(out)).unwrap()
}
