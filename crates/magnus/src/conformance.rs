//! The adapter conformance suite, run against the live provider with the
//! vault's credential: contract handshake, catalog, streaming, a tool-call
//! round trip carrying the opaque state, served-model reporting, sign-in and
//! refresh, and every error class. Two classes cannot be caused safely on a
//! live account (a malformed response, a usage limit); those run against a
//! local fake and are marked simulated. An adapter ships only when every
//! check passes.

use crate::config;
use crate::vault::Vault;
use anyhow::Result;
use magnus_adapter_proto::{self as proto, Block, CompleteParams, ErrorClass, Event, Message, Request, Role, ToolSpec};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

/// The longest an adapter may go without an event during a conformance call.
const EVENT_WAIT: Duration = Duration::from_secs(300);

#[derive(Serialize, Clone, Debug)]
pub struct Check {
    pub name: String,
    pub passed: bool,
    pub simulated: bool,
    pub detail: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct Report {
    pub adapter_id: String,
    pub adapter: String,
    pub version: String,
    pub model: String,
    pub passed: bool,
    pub checks: Vec<Check>,
}

struct Outcome {
    events: Vec<Event>,
}

impl Outcome {
    fn error(&self) -> Option<(ErrorClass, String)> {
        self.events.iter().find_map(|e| match e {
            Event::Error { class, message, .. } => Some((*class, message.clone())),
            _ => None,
        })
    }
    fn text(&self) -> String {
        self.events.iter().filter_map(|e| if let Event::Text { text } = e { Some(text.as_str()) } else { None }).collect()
    }
    fn texts(&self) -> usize {
        self.events.iter().filter(|e| matches!(e, Event::Text { .. })).count()
    }
    fn calls(&self) -> Vec<(String, String, Value)> {
        self.events.iter().filter_map(|e| if let Event::ToolCall { id, name, input } = e { Some((id.clone(), name.clone(), input.clone())) } else { None }).collect()
    }
    fn state(&self) -> Option<Value> {
        self.events.iter().find_map(|e| if let Event::State { state } = e { Some(state.clone()) } else { None })
    }
    fn served(&self) -> Option<String> {
        self.events.iter().find_map(|e| if let Event::ServedModel { id } = e { Some(id.clone()) } else { None })
    }
    fn usage(&self) -> Option<proto::Usage> {
        self.events.iter().find_map(|e| if let Event::Usage(u) = e { Some(u.clone()) } else { None })
    }
    fn refreshed(&self) -> Option<String> {
        self.events.iter().find_map(|e| if let Event::Credential { value } = e { Some(value.clone()) } else { None })
    }
    fn done(&self) -> bool {
        self.events.iter().any(|e| matches!(e, Event::Done { .. }))
    }
}

async fn call(socket: &Path, req: Request) -> Result<Outcome> {
    let mut c = proto::Call::open(socket, req).await?;
    let mut events = vec![];
    loop {
        match tokio::time::timeout(EVENT_WAIT, c.next()).await {
            Ok(Ok(Some(e))) => {
                let end = matches!(e, Event::Done { .. } | Event::Error { .. } | Event::Hello { .. } | Event::Catalog { .. } | Event::AuthDone { .. } | Event::AuthStep { .. });
                events.push(e);
                if end {
                    break;
                }
            }
            Ok(Ok(None)) => break,
            Ok(Err(e)) => return Err(e),
            Err(_) => anyhow::bail!("no answer within 300s"),
        }
    }
    Ok(Outcome { events })
}

fn params(model: &str, settings: &Value, credential: Option<String>, messages: Vec<Message>, tools: Vec<ToolSpec>) -> CompleteParams {
    CompleteParams {
        model: model.into(),
        settings: settings.clone(),
        credential,
        system: "You are being tested. Follow the user's instruction exactly.".into(),
        messages,
        tools,
        max_tokens: 2048,
    }
}

fn user(t: &str) -> Message {
    Message { role: Role::User, content: vec![Block::Text { text: t.into() }], state: None }
}

/// A local stand-in provider for the two error classes a live account cannot
/// safely produce.
async fn fake_provider() -> Result<String> {
    use axum::{response::IntoResponse, Router};
    async fn any(uri: axum::http::Uri) -> axum::response::Response {
        if uri.path().starts_with("/limit") {
            let body = json!({ "type": "error", "error": { "type": "rate_limit_error", "code": "rate_limit_exceeded", "message": "simulated usage limit" } });
            return (axum::http::StatusCode::TOO_MANY_REQUESTS, axum::Json(body)).into_response();
        }
        ([("content-type", "text/event-stream")], "data: {this is not json\n\n").into_response()
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        let _ = axum::serve(listener, Router::new().fallback(any)).await;
    });
    Ok(format!("http://{addr}"))
}

pub async fn run(vault: &Vault, cfg: &config::Adapter, running_socket: &Path, adapter: &str, version: &str, model: &str) -> Report {
    let mut checks = vec![];
    let mut push = |name: &str, passed: bool, simulated: bool, detail: String| {
        checks.push(Check { name: name.into(), passed, simulated, detail });
    };
    let mut settings = serde_json::Map::from_iter(cfg.settings.clone());
    let cred_name = settings.remove("credential").and_then(|v| v.as_str().map(String::from));
    let settings = Value::Object(settings);
    let credential = || cred_name.as_ref().and_then(|n| vault.get(n).ok().flatten()).map(|s| s.expose().to_string());
    let store = |value: &str| {
        if let Some(n) = &cred_name {
            let _ = vault.put(n, value);
        }
    };
    let oauth = settings["auth"]["kind"].as_str().is_some_and(|k| k.starts_with("oauth"));

    // Contract handshake.
    match call(running_socket, Request::Hello {}).await {
        Ok(o) => {
            let ok = o.events.iter().any(|e| matches!(e, Event::Hello { contract, .. } if contract == proto::CONTRACT));
            push("contract", ok, false, format!("{:?}", o.events.first()));
        }
        Err(e) => push("contract", false, false, e.to_string()),
    }

    // Sign-in and refresh.
    if credential().is_none() {
        push("sign_in", false, false, "no credential in the vault: sign in first".into());
    } else if oauth {
        let mut c: Value = serde_json::from_str(&credential().unwrap_or_default()).unwrap_or(Value::Null);
        c["expires_at"] = json!(0);
        let p = params(model, &settings, Some(c.to_string()), vec![user("Reply with the word ok.")], vec![]);
        match call(running_socket, Request::Complete(p)).await {
            Ok(o) => {
                if let Some(r) = o.refreshed() {
                    store(&r);
                }
                let ok = o.refreshed().is_some() && o.done();
                push("sign_in_and_refresh", ok, false, format!("refreshed: {}, completed: {}, error: {:?}", o.refreshed().is_some(), o.done(), o.error()));
            }
            Err(e) => push("sign_in_and_refresh", false, false, e.to_string()),
        }
    } else {
        let flow = json!({ "kind": "api_key" });
        let input = json!({ "api_key": credential().unwrap_or_default() });
        match call(running_socket, Request::AuthComplete { settings: settings.clone(), flow, input }).await {
            Ok(o) => {
                let ok = o.events.iter().any(|e| matches!(e, Event::AuthDone { .. }));
                push("sign_in", ok, false, format!("{:?}", o.error()));
            }
            Err(e) => push("sign_in", false, false, e.to_string()),
        }
    }

    // Catalog.
    match call(running_socket, Request::Catalog { settings: settings.clone(), credential: credential() }).await {
        Ok(o) => {
            if let Some(r) = o.refreshed() {
                store(&r);
            }
            match o.events.iter().find_map(|e| if let Event::Catalog { models } = e { Some(models.clone()) } else { None }) {
                Some(models) => {
                    let listed = models.iter().any(|m| m.id == model);
                    push(
                        "catalog",
                        !models.is_empty(),
                        false,
                        format!("{} models; configured model listed: {listed}; fixed versions: {}", models.len(), models.iter().filter(|m| m.fixed_version).count()),
                    );
                }
                None => push("catalog", false, false, format!("{:?}", o.error())),
            }
        }
        Err(e) => push("catalog", false, false, e.to_string()),
    }

    // Streaming and served model.
    let p = params(model, &settings, credential(), vec![user("Count from one to ten in words, separated by spaces. Nothing else.")], vec![]);
    match call(running_socket, Request::Complete(p)).await {
        Ok(o) => {
            if let Some(r) = o.refreshed() {
                store(&r);
            }
            let text = o.text().to_lowercase();
            push(
                "streaming",
                o.done() && o.texts() >= 2 && text.contains("ten"),
                false,
                format!("{} text events; text {:?}; error {:?}", o.texts(), o.text().chars().take(80).collect::<String>(), o.error()),
            );
            let served = o.served().unwrap_or_default();
            push("served_model", !served.is_empty(), false, format!("asked for {model}, served {served}"));
            let u = o.usage();
            push("usage", u.as_ref().is_some_and(|u| u.output_tokens > 0), false, format!("{u:?}"));
        }
        Err(e) => push("streaming", false, false, e.to_string()),
    }

    // Tool-call round trip with state.
    let tool = ToolSpec {
        name: "get_temperature".into(),
        description: "Get the current temperature in a city.".into(),
        input_schema: json!({ "type": "object", "properties": { "city": { "type": "string" } }, "required": ["city"] }),
    };
    let first = vec![user("What is the temperature in Oslo right now? Use the get_temperature tool, then tell me the number.")];
    let p = params(model, &settings, credential(), first.clone(), vec![tool.clone()]);
    match call(running_socket, Request::Complete(p)).await {
        Ok(o) => {
            let calls = o.calls();
            let ok_call = calls.iter().any(|(_, n, i)| n == "get_temperature" && i["city"].as_str().unwrap_or_default().to_lowercase().contains("oslo"));
            push("tool_call", ok_call, false, format!("{calls:?} {:?}", o.error()));
            if let Some((id, name, input)) = calls.first().cloned() {
                let mut assistant = vec![];
                if !o.text().is_empty() {
                    assistant.push(Block::Text { text: o.text() });
                }
                assistant.push(Block::ToolCall { id: id.clone(), name, input });
                let state = o.state();
                let mut msgs = first.clone();
                msgs.push(Message { role: Role::Assistant, content: assistant, state: state.clone() });
                msgs.push(Message {
                    role: Role::User,
                    content: vec![Block::ToolResult { call_id: id, content: vec![Block::Text { text: "-7 degrees Celsius".into() }], is_error: false }],
                    state: None,
                });
                let p = params(model, &settings, credential(), msgs, vec![tool.clone()]);
                match call(running_socket, Request::Complete(p)).await {
                    Ok(o2) => {
                        push(
                            "tool_result_round_trip",
                            o2.done() && o2.text().contains('7'),
                            false,
                            format!("answer {:?}; error {:?}", o2.text().chars().take(120).collect::<String>(), o2.error()),
                        );
                        push("state_round_trip", o2.done(), false, format!("state from turn one: {}; second turn accepted it: {}", state.is_some(), o2.done()));
                    }
                    Err(e) => push("tool_result_round_trip", false, false, e.to_string()),
                }
            }
        }
        Err(e) => push("tool_call", false, false, e.to_string()),
    }

    // Error classes.
    let expect = |o: &Result<Outcome>, want: ErrorClass| -> (bool, String) {
        match o {
            Ok(o) => match o.error() {
                Some((c, m)) => (c == want, format!("{c:?}: {m}")),
                None => (false, "no error".into()),
            },
            Err(e) => (false, e.to_string()),
        }
    };
    let bogus = if oauth {
        json!({ "access_token": "magnus-conformance-invalid", "refresh_token": null, "expires_at": 4102444800i64 }).to_string()
    } else {
        "magnus-conformance-invalid-key".to_string()
    };
    let o = call(running_socket, Request::Complete(params(model, &settings, Some(bogus), vec![user("hi")], vec![]))).await;
    let (ok, d) = expect(&o, ErrorClass::AuthExpired);
    push("error_auth_expired", ok, false, d);

    let o = call(running_socket, Request::Complete(params("magnus-conformance-no-such-model", &settings, credential(), vec![user("hi")], vec![]))).await;
    let (ok, d) = expect(&o, ErrorClass::InvalidRequest);
    push("error_invalid_request", ok, false, d);

    let mut down = settings.clone();
    down["base_url"] = json!("http://127.0.0.1:9");
    let o = call(running_socket, Request::Complete(params(model, &down, credential(), vec![user("hi")], vec![]))).await;
    let (ok, d) = expect(&o, ErrorClass::Unavailable);
    push("error_unavailable", ok, false, d);

    match fake_provider().await {
        Ok(base) => {
            let mut s = settings.clone();
            s["base_url"] = json!(format!("{base}/garbage"));
            if s["auth"]["kind"].as_str().is_some_and(|k| k.starts_with("oauth")) {
                s["auth"] = json!({ "kind": "api_key" });
            }
            let o = call(running_socket, Request::Complete(params(model, &s, Some("k".into()), vec![user("hi")], vec![]))).await;
            let (ok, d) = expect(&o, ErrorClass::Malformed);
            push("error_malformed", ok, true, d);
            s["base_url"] = json!(format!("{base}/limit"));
            let o = call(running_socket, Request::Complete(params(model, &s, Some("k".into()), vec![user("hi")], vec![]))).await;
            let (ok, d) = expect(&o, ErrorClass::UsageLimit);
            push("error_usage_limit", ok, true, d);
        }
        Err(e) => push("error_malformed", false, true, e.to_string()),
    }

    let passed = checks.iter().all(|c| c.passed);
    Report { adapter_id: cfg.id.clone(), adapter: adapter.into(), version: version.into(), model: model.into(), passed, checks }
}
