//! Chat Completions (`POST {base}/chat/completions`, streamed).
//! Opaque state per assistant turn: `{ "reasoning_content": ... }` when the
//! provider streams reasoning (GLM); sent back only with `send_reasoning`.

use crate::{bearer, classify, finish, request, retry_after, tool_specs, zero_usage, Sse};
use magnus_adapter_proto::{Block, CompleteParams, ErrorClass, Event, Role, Sink};
use serde_json::{json, Value};

fn text_of(content: &[Block]) -> String {
    content
        .iter()
        .filter_map(|b| match b {
            Block::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn messages(p: &CompleteParams) -> Vec<Value> {
    let mut out = vec![json!({ "role": "system", "content": p.system })];
    let send_reasoning = p.settings["send_reasoning"].as_bool().unwrap_or(false);
    for m in &p.messages {
        match m.role {
            Role::User => {
                // Tool results become `tool` messages; the rest one user message.
                let mut parts = vec![];
                for b in &m.content {
                    match b {
                        Block::ToolResult { call_id, content, is_error } => {
                            let t = text_of(content);
                            out.push(json!({ "role": "tool", "tool_call_id": call_id, "content": if *is_error { format!("ERROR: {t}") } else { t } }));
                            // Tool messages carry text only; images follow in the user turn.
                            for c in content {
                                if let Block::Image { media_type, data_b64 } = c {
                                    parts.push(json!({ "type": "text", "text": format!("Image from tool call {call_id}:") }));
                                    parts.push(json!({ "type": "image_url", "image_url": { "url": format!("data:{media_type};base64,{data_b64}") } }));
                                }
                            }
                        }
                        Block::Text { text } => parts.push(json!({ "type": "text", "text": text })),
                        Block::Image { media_type, data_b64 } => parts.push(json!({ "type": "image_url", "image_url": { "url": format!("data:{media_type};base64,{data_b64}") } })),
                        Block::Document { name, .. } => parts.push(json!({ "type": "text", "text": format!("[attached document {name}; read it with your tools]") })),
                        Block::ToolCall { .. } => {}
                    }
                }
                if !parts.is_empty() {
                    out.push(json!({ "role": "user", "content": parts }));
                }
            }
            Role::Assistant => {
                let calls: Vec<Value> = m
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        Block::ToolCall { id, name, input } => Some(json!({ "id": id, "type": "function", "function": { "name": name, "arguments": input.to_string() } })),
                        _ => None,
                    })
                    .collect();
                let mut msg = json!({ "role": "assistant", "content": text_of(&m.content) });
                if !calls.is_empty() {
                    msg["tool_calls"] = json!(calls);
                }
                if send_reasoning {
                    if let Some(r) = m.state.as_ref().and_then(|s| s["reasoning_content"].as_str()) {
                        msg["reasoning_content"] = json!(r);
                    }
                }
                out.push(msg);
            }
        }
    }
    out
}

/// The request body a turn sends: one builder for the asking and the
/// measuring, so the estimate can never drift from what goes out.
pub fn request_body(p: &CompleteParams) -> Value {
    let mut body = json!({
        "model": p.model,
        "messages": messages(p),
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if !p.tools.is_empty() {
        body["tools"] = json!(tool_specs(&p.tools, false));
    }
    match p.settings.get("max_tokens_field") {
        Some(Value::Null) | Some(Value::Bool(false)) => {}
        Some(Value::String(f)) => body[f.as_str()] = json!(p.max_tokens),
        _ => body["max_tokens"] = json!(p.max_tokens),
    }
    if let Some(extra) = p.settings["body"].as_object() {
        for (k, v) in extra {
            body[k.as_str()] = v.clone();
        }
    }
    body
}

pub async fn complete(http: &reqwest::Client, p: CompleteParams, sink: &mut Sink) -> anyhow::Result<()> {
    let body = request_body(&p);
    let mut force = false;
    let resp = loop {
        let (token, account, refreshed) = match bearer(http, &p.settings, &p.credential, force).await {
            Ok(t) => t,
            Err((c, m)) => return sink.error(c, m).await,
        };
        if let Some(r) = refreshed {
            sink.send(&Event::Credential { value: r }).await?;
        }
        let r = match request(http, &p.settings, "/chat/completions", &token, &account).json(&body).send().await {
            Ok(r) => r,
            Err(e) => return sink.error(ErrorClass::Unavailable, format!("could not reach the provider: {e}")).await,
        };
        let status = r.status().as_u16();
        if status == 401 && !force && p.settings["auth"]["kind"].as_str().unwrap_or("api_key") != "api_key" {
            force = true;
            continue;
        }
        if !(200..300).contains(&status) {
            let h = r.headers().clone();
            let text = r.text().await.unwrap_or_default();
            let (class, message) = classify(status, &text);
            return sink.send(&Event::Error { class, message, retry_after_s: retry_after(&h, &text) }).await;
        }
        break r;
    };

    let mut text = String::new();
    let mut reasoning = String::new();
    let mut calls: Vec<(String, String, String)> = vec![];
    let mut usage = zero_usage();
    let mut served: Option<String> = None;
    let mut stop = "end_turn".to_string();
    let mut stream = Sse::new(resp);
    let r: Result<(), (ErrorClass, String)> = loop {
        let v = match stream.next().await {
            None => break Ok(()),
            Some(Err(e)) => break Err(e),
            Some(Ok(v)) => v,
        };
        if let Some(m) = v["model"].as_str() {
            served = Some(m.to_string());
        }
        if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
            break Err((ErrorClass::Unavailable, format!("stream error: {err}")));
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            usage.input_tokens = u["prompt_tokens"].as_u64().unwrap_or(0);
            usage.output_tokens = u["completion_tokens"].as_u64().unwrap_or(0);
            usage.cache_read_tokens = u["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0);
        }
        for c in v["choices"].as_array().into_iter().flatten() {
            let d = &c["delta"];
            if let Some(t) = d["content"].as_str() {
                text.push_str(t);
                sink.send(&Event::Text { text: t.to_string() }).await?;
            }
            if let Some(t) = d["reasoning_content"].as_str() {
                reasoning.push_str(t);
            }
            for tc in d["tool_calls"].as_array().into_iter().flatten() {
                let i = tc["index"].as_u64().unwrap_or(calls.len() as u64) as usize;
                while calls.len() <= i {
                    calls.push(Default::default());
                }
                if let Some(id) = tc["id"].as_str() {
                    calls[i].0 = id.to_string();
                }
                if let Some(n) = tc["function"]["name"].as_str() {
                    calls[i].1.push_str(n);
                }
                if let Some(a) = tc["function"]["arguments"].as_str() {
                    calls[i].2.push_str(a);
                }
            }
            if let Some(f) = c["finish_reason"].as_str() {
                stop = match f {
                    "tool_calls" => "tool_use".into(),
                    "length" => "max_tokens".into(),
                    _ => "end_turn".into(),
                };
            }
        }
    };
    if let Err((c, m)) = r {
        return sink.error(c, m).await;
    }
    for (i, (id, name, args)) in calls.into_iter().enumerate() {
        let input: Value = if args.trim().is_empty() {
            json!({})
        } else {
            match serde_json::from_str(&args) {
                Ok(v) => v,
                Err(_) => return sink.error(ErrorClass::Malformed, format!("tool call {name} has arguments that are not JSON")).await,
            }
        };
        let id = if id.is_empty() { format!("call_{i}") } else { id };
        sink.send(&Event::ToolCall { id, name, input }).await?;
    }
    if !reasoning.is_empty() {
        sink.send(&Event::State { state: json!({ "reasoning_content": reasoning }) }).await?;
    }
    finish(sink, &p.settings, &p.model, usage, served, &stop).await
}
