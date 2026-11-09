//! Fake MCP servers (streamable HTTP transport, JSON responses) standing in for
//! the person's apps. Each keeps a `world` that tools read and change, and
//! records every `tools/call` it receives with its headers.
//!
//! When built with `honor_idempotency`, a call carrying an `Idempotency-Key`
//! header that was seen before with the same arguments returns the stored
//! response without repeating the effect, as a well-behaved API would.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use serde_json::{json, Map, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

pub type World = Map<String, Value>;

#[derive(Clone, Debug)]
pub struct Call {
    pub tool: String,
    pub args: Value,
    pub headers: HashMap<String, String>,
    /// Whether this call ran the tool's effect (false when deduplicated).
    pub effective: bool,
}

impl Call {
    pub fn idempotency_key(&self) -> Option<&str> {
        self.headers.get("idempotency-key").map(String::as_str)
    }
    pub fn bearer(&self) -> Option<&str> {
        self.headers.get("authorization").and_then(|v| v.strip_prefix("Bearer "))
    }
}

pub struct Out {
    pub structured: Value,
    pub is_error: bool,
}

pub fn ok(v: Value) -> Out {
    Out { structured: v, is_error: false }
}

pub fn err(msg: &str) -> Out {
    Out { structured: json!({ "error": msg }), is_error: true }
}

type Handler = Arc<dyn Fn(&mut World, &Value, &HashMap<String, String>) -> Out + Send + Sync>;

#[derive(Clone)]
struct ToolDef {
    name: String,
    description: String,
    schema: Value,
    handler: Handler,
}

struct Hold {
    tool: String,
    received: Arc<Notify>,
    release: Arc<Notify>,
}

struct Inner {
    tools: Mutex<Vec<ToolDef>>,
    world: Mutex<World>,
    calls: Mutex<Vec<Call>>,
    seen: Mutex<HashMap<String, (Value, Value)>>,
    holds: Mutex<Vec<Hold>>,
    honor_keys: bool,
}

pub struct Builder {
    tools: Vec<ToolDef>,
    world: World,
    honor_keys: bool,
}

impl Builder {
    pub fn tool<F>(mut self, name: &str, description: &str, schema: Value, f: F) -> Self
    where
        F: Fn(&mut World, &Value, &HashMap<String, String>) -> Out + Send + Sync + 'static,
    {
        self.tools.push(ToolDef { name: name.into(), description: description.into(), schema, handler: Arc::new(f) });
        self
    }

    pub fn world(mut self, key: &str, v: Value) -> Self {
        self.world.insert(key.into(), v);
        self
    }

    pub fn honor_idempotency(mut self, yes: bool) -> Self {
        self.honor_keys = yes;
        self
    }

    pub async fn start(self) -> FakeMcp {
        let inner = Arc::new(Inner {
            tools: Mutex::new(self.tools),
            world: Mutex::new(self.world),
            calls: Mutex::new(vec![]),
            seen: Mutex::new(HashMap::new()),
            holds: Mutex::new(vec![]),
            honor_keys: self.honor_keys,
        });
        let app = Router::new().route("/mcp", post(rpc)).with_state(inner.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        FakeMcp { url: format!("http://{addr}/mcp"), port: addr.port(), inner }
    }
}

#[derive(Clone)]
pub struct FakeMcp {
    pub url: String,
    pub port: u16,
    inner: Arc<Inner>,
}

pub struct HoldHandle {
    received: Arc<Notify>,
    release: Arc<Notify>,
}

impl HoldHandle {
    /// Resolves once the held call has arrived and its effect has been applied.
    pub async fn received(&self) {
        self.received.notified().await
    }
    /// Let the held response go (to whoever is still listening).
    pub fn release(&self) {
        self.release.notify_one()
    }
}

impl FakeMcp {
    pub fn builder() -> Builder {
        Builder { tools: vec![], world: Map::new(), honor_keys: false }
    }

    pub fn calls(&self) -> Vec<Call> {
        self.inner.calls.lock().unwrap().clone()
    }

    pub fn calls_to(&self, tool: &str) -> Vec<Call> {
        self.calls().into_iter().filter(|c| c.tool == tool).collect()
    }

    /// How many times `tool` actually changed the world.
    pub fn effects(&self, tool: &str) -> usize {
        self.calls_to(tool).iter().filter(|c| c.effective).count()
    }

    pub fn world(&self) -> World {
        self.inner.world.lock().unwrap().clone()
    }

    pub fn set_world(&self, key: &str, v: Value) {
        self.inner.world.lock().unwrap().insert(key.into(), v);
    }

    /// The next call to `tool` applies its effect, then withholds its response
    /// until released: a lost response after a real side effect.
    pub fn hold(&self, tool: &str) -> HoldHandle {
        let received = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        self.inner.holds.lock().unwrap().push(Hold { tool: tool.into(), received: received.clone(), release: release.clone() });
        HoldHandle { received, release }
    }

    /// Change a tool's advertised description, as a compromised server would.
    pub fn set_description(&self, tool: &str, description: &str) {
        for t in self.inner.tools.lock().unwrap().iter_mut() {
            if t.name == tool {
                t.description = description.into();
            }
        }
    }
}

async fn rpc(State(inner): State<Arc<Inner>>, headers: HeaderMap, body: String) -> Response {
    let req: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let Some(id) = req.get("id").cloned() else {
        return StatusCode::ACCEPTED.into_response(); // a notification
    };
    let headers: HashMap<String, String> = headers.iter().map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or_default().to_string())).collect();
    let method = req["method"].as_str().unwrap_or_default();
    let result = match method {
        "initialize" => json!({
            "protocolVersion": req["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "fake", "version": "1.0.0" }
        }),
        "ping" => json!({}),
        "tools/list" => {
            let tools: Vec<Value> = inner.tools.lock().unwrap().iter().map(|t| json!({ "name": t.name, "description": t.description, "inputSchema": t.schema })).collect();
            json!({ "tools": tools })
        }
        "tools/call" => call(&inner, &req["params"], headers).await,
        _ => {
            return axum::Json(json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": format!("unknown method {method}") }
            }))
            .into_response()
        }
    };
    axum::Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
}

async fn call(inner: &Arc<Inner>, params: &Value, headers: HashMap<String, String>) -> Value {
    let name = params["name"].as_str().unwrap_or_default().to_string();
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let key = headers.get("idempotency-key").cloned();

    if inner.honor_keys {
        if let Some(k) = &key {
            let seen = inner.seen.lock().unwrap().get(k).cloned();
            if let Some((prev_args, prev_result)) = seen {
                inner.calls.lock().unwrap().push(Call { tool: name, args: args.clone(), headers, effective: false });
                if prev_args == args {
                    return prev_result;
                }
                return render(err("idempotency key reused with different arguments"));
            }
        }
    }

    let def = inner.tools.lock().unwrap().iter().find(|t| t.name == name).cloned();
    let out = match &def {
        Some(d) => {
            let mut world = inner.world.lock().unwrap();
            (d.handler)(&mut world, &args, &headers)
        }
        None => err("no such tool"),
    };
    let result = render(out);
    inner.calls.lock().unwrap().push(Call { tool: name.clone(), args: args.clone(), headers, effective: def.is_some() });
    if inner.honor_keys {
        if let Some(k) = key {
            inner.seen.lock().unwrap().insert(k, (args, result.clone()));
        }
    }

    let hold = {
        let mut holds = inner.holds.lock().unwrap();
        holds.iter().position(|h| h.tool == name).map(|i| holds.remove(i))
    };
    if let Some(h) = hold {
        h.received.notify_one();
        h.release.notified().await;
    }
    result
}

fn render(out: Out) -> Value {
    json!({
        "content": [{ "type": "text", "text": out.structured.to_string() }],
        "structuredContent": out.structured,
        "isError": out.is_error
    })
}

fn obj(props: Value) -> Value {
    json!({ "type": "object", "properties": props })
}

/// The person's mailbox. `list_inbox` returns private mail; `send` is the
/// external action; `list_sent` lets a contract check the world; `whoami`
/// reflects the credential it was called with, as a careless API might.
pub fn mail() -> Builder {
    FakeMcp::builder()
        .world("inbox", json!([{ "from": "cfo@corp.example", "subject": "Q3 numbers", "body": "Confidential: revenue 41.7M" }]))
        .world("sent", json!([]))
        .honor_idempotency(true)
        .tool("list_inbox", "List messages in the inbox.", obj(json!({})), |w, _, _| ok(json!({ "messages": w["inbox"] })))
        .tool("send", "Send an email.", obj(json!({ "to": {"type": "string"}, "subject": {"type": "string"}, "body": {"type": "string"} })), |w, a, _| {
            w["sent"].as_array_mut().unwrap().push(a.clone());
            ok(json!({ "status": "sent" }))
        })
        .tool("list_sent", "List sent messages.", obj(json!({})), |w, _, _| {
            let sent = w["sent"].as_array().unwrap();
            ok(json!({ "count": sent.len(), "messages": sent }))
        })
        .tool("whoami", "Show the calling account.", obj(json!({})), |_, _, h| {
            ok(json!({ "account": "me", "debug_authorization": h.get("authorization").cloned().unwrap_or_default() }))
        })
}

/// A record store. Record 42 starts open.
pub fn crm() -> Builder {
    FakeMcp::builder()
        .world("records", json!({ "42": { "id": "42", "status": "open" } }))
        .honor_idempotency(true)
        .tool("get_record", "Get a record by id.", obj(json!({ "id": {"type": "string"} })), |w, a, _| {
            let id = a["id"].as_str().unwrap_or_default();
            match w["records"].get(id) {
                Some(r) => ok(r.clone()),
                None => err("no such record"),
            }
        })
        .tool("update_record", "Update fields on a record.", obj(json!({ "id": {"type": "string"}, "fields": {"type": "object"} })), |w, a, _| {
            let id = a["id"].as_str().unwrap_or_default().to_string();
            let Some(rec) = w["records"].get_mut(&id) else { return err("no such record") };
            if let (Some(r), Some(f)) = (rec.as_object_mut(), a["fields"].as_object()) {
                for (k, v) in f {
                    r.insert(k.clone(), v.clone());
                }
            }
            ok(rec.clone())
        })
        .tool("delete_all", "Delete every record.", obj(json!({})), |w, _, _| {
            w.insert("records".into(), json!({}));
            ok(json!({ "deleted": true }))
        })
}

/// Team chat, an internal audience.
pub fn chat() -> Builder {
    FakeMcp::builder()
        .world("posts", json!([]))
        .honor_idempotency(true)
        .tool("post", "Post a message to a channel.", obj(json!({ "channel": {"type": "string"}, "text": {"type": "string"} })), |w, a, _| {
            w["posts"].as_array_mut().unwrap().push(a.clone());
            ok(json!({ "status": "posted" }))
        })
        .tool("list_posts", "List posted messages.", obj(json!({})), |w, _, _| {
            let posts = w["posts"].as_array().unwrap();
            ok(json!({ "count": posts.len(), "posts": posts }))
        })
}

/// The person's own notes, a private audience.
pub fn notes() -> Builder {
    FakeMcp::builder().world("notes", json!([])).honor_idempotency(true).tool(
        "save",
        "Save a note.",
        obj(json!({ "title": {"type": "string"}, "text": {"type": "string"} })),
        |w, a, _| {
            w["notes"].as_array_mut().unwrap().push(a.clone());
            ok(json!({ "status": "saved" }))
        },
    )
}

/// Finance reports, restricted to the finance group.
pub fn finance() -> Builder {
    FakeMcp::builder()
        .honor_idempotency(true)
        .tool("get_report", "Get a quarterly report.", obj(json!({ "quarter": {"type": "string"} })), |_, a, _| ok(json!({ "quarter": a["quarter"], "revenue": "41.7M" })))
}
