//! Responses (`POST {base}/responses`, streamed), used statelessly: `store`
//! false and the full input every turn. Opaque state per assistant turn:
//! `{ "items": [...] }`, the turn's reasoning items (with encrypted content),
//! sent back in front of that turn's output.

use crate::{fail, finish, post_signed_in, tool_specs, zero_usage, Sse};
use magnus_adapter_proto::{Block, CompleteParams, ErrorClass, Event, Role, Sink, Usage};
use serde_json::{json, Value};

fn input(p: &CompleteParams) -> Vec<Value> {
    let mut out = vec![];
    for m in &p.messages {
        match m.role {
            Role::User => {
                let mut parts = vec![];
                for b in &m.content {
                    match b {
                        Block::ToolResult { call_id, content, is_error } => {
                            let t: Vec<&str> = content.iter().filter_map(|c| if let Block::Text { text } = c { Some(text.as_str()) } else { None }).collect();
                            let t = t.join("\n");
                            out.push(json!({ "type": "function_call_output", "call_id": call_id, "output": if *is_error { format!("ERROR: {t}") } else { t } }));
                            for c in content {
                                if let Block::Image { media_type, data_b64 } = c {
                                    parts.push(json!({ "type": "input_text", "text": format!("Image from tool call {call_id}:") }));
                                    parts.push(json!({ "type": "input_image", "image_url": format!("data:{media_type};base64,{data_b64}") }));
                                }
                            }
                        }
                        Block::Text { text } => parts.push(json!({ "type": "input_text", "text": text })),
                        Block::Image { media_type, data_b64 } => parts.push(json!({ "type": "input_image", "image_url": format!("data:{media_type};base64,{data_b64}") })),
                        Block::Document { media_type, data_b64, name } => {
                            parts.push(json!({ "type": "input_file", "filename": name, "file_data": format!("data:{media_type};base64,{data_b64}") }))
                        }
                        Block::ToolCall { .. } => {}
                    }
                }
                if !parts.is_empty() {
                    out.push(json!({ "type": "message", "role": "user", "content": parts }));
                }
            }
            Role::Assistant => {
                if let Some(items) = m.state.as_ref().and_then(|s| s["items"].as_array()) {
                    out.extend(items.iter().cloned());
                }
                for b in &m.content {
                    match b {
                        Block::Text { text } if !text.is_empty() => {
                            out.push(json!({ "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": text }] }))
                        }
                        Block::ToolCall { id, name, input } => out.push(json!({ "type": "function_call", "call_id": id, "name": name, "arguments": input.to_string() })),
                        _ => {}
                    }
                }
            }
        }
    }
    out
}

/// One key for every turn of one conversation: the same model,
/// instructions and opening item. The provider routes turns with the same
/// key to the same cache, so each turn's prefix is read from it instead of
/// charged again; without it turns scatter and most miss.
fn cache_key(model: &str, system: &str, first: Option<&Value>) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (model, system, first.map(Value::to_string)).hash(&mut h);
    format!("magnus-{:016x}", h.finish())
}

/// The request body a turn sends: one builder for the asking and the
/// measuring, so the estimate can never drift from what goes out.
pub fn request_body(p: &CompleteParams) -> Value {
    let s = &p.settings;
    let input = input(p);
    // No prefix: what the provider is sent is what the kernel logged, with
    // no appliance-config text outside the version (#269).
    let mut body = json!({
        "model": p.model,
        "instructions": p.system,
        "stream": true,
        "store": s["store"].as_bool().unwrap_or(false),
        "tool_choice": "auto",
        // Several reads in one reply: each extra reply resends the context.
        "parallel_tool_calls": s["parallel_tool_calls"].as_bool().unwrap_or(true),
    });
    if s["prompt_cache_key"].as_bool() != Some(false) {
        body["prompt_cache_key"] = json!(cache_key(&p.model, &p.system, input.first()));
    }
    body["input"] = json!(input);
    if let Some(inc) = s.get("include").filter(|v| v.is_array()) {
        body["include"] = inc.clone();
    } else {
        body["include"] = json!(["reasoning.encrypted_content"]);
    }
    if !p.tools.is_empty() {
        body["tools"] = json!(tool_specs(&p.tools, true));
    }
    match s.get("max_tokens_field") {
        Some(Value::Null) | Some(Value::Bool(false)) => {}
        Some(Value::String(f)) => body[f.as_str()] = json!(p.max_tokens),
        _ => body["max_output_tokens"] = json!(p.max_tokens),
    }
    if let Some(effort) = s["reasoning_effort"].as_str().filter(|e| matches!(*e, "low" | "medium" | "high")) {
        body["reasoning"] = json!({ "effort": effort });
    }
    if let Some(extra) = s["body"].as_object() {
        for (k, v) in extra {
            body[k.as_str()] = v.clone();
        }
    }
    body
}

/// The counts a response reports, when it reports them.
fn read_usage(usage: &mut Usage, u: &Value) {
    if !u.is_object() {
        return;
    }
    usage.input_tokens = u["input_tokens"].as_u64().unwrap_or(0);
    usage.output_tokens = u["output_tokens"].as_u64().unwrap_or(0);
    usage.cache_read_tokens = u["input_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0);
}

/// The headers that name the conversation, as Codex sends them: the
/// ChatGPT backend picks a turn's cache by these, not by the body's key
/// alone. With only the body's key, 31 turns after 0.2.6-test.333 read
/// nothing from cache.
fn conversation_headers(body: &Value) -> Vec<(&'static str, String)> {
    match body["prompt_cache_key"].as_str() {
        Some(k) => vec![("session_id", k.to_string()), ("x-client-request-id", k.to_string())],
        None => vec![],
    }
}

pub async fn complete(http: &reqwest::Client, p: CompleteParams, sink: &mut Sink) -> anyhow::Result<()> {
    let s = &p.settings;
    let body = request_body(&p);
    let headers = conversation_headers(&body);
    let shape = |r: reqwest::RequestBuilder| headers.iter().fold(r.header("accept", "text/event-stream"), |r, (k, v)| r.header(*k, v));
    let Some(resp) = post_signed_in(http, s, &p.credential, "/responses", &body, shape, sink).await? else {
        return Ok(());
    };

    let mut calls = vec![];
    let mut reasoning_items = vec![];
    let mut usage = zero_usage();
    let mut served: Option<String> = None;
    let mut stop = "end_turn".to_string();
    let mut completed = false;
    let mut stream = Sse::new(resp);
    let r: Result<(), (ErrorClass, String)> = loop {
        let v = match stream.next().await {
            None => break Ok(()),
            Some(Err(e)) => break Err(e),
            Some(Ok(v)) => v,
        };
        match v["type"].as_str().unwrap_or_default() {
            "response.output_text.delta" => sink.send(&Event::Text { text: v["delta"].as_str().unwrap_or_default().to_string() }).await?,
            "response.output_item.done" => {
                let item = &v["item"];
                match item["type"].as_str().unwrap_or_default() {
                    "function_call" => calls.push((
                        item["call_id"].as_str().unwrap_or_default().to_string(),
                        item["name"].as_str().unwrap_or_default().to_string(),
                        item["arguments"].as_str().unwrap_or_default().to_string(),
                    )),
                    "reasoning" => {
                        let mut it = item.clone();
                        if let Some(o) = it.as_object_mut() {
                            o.remove("id");
                            o.remove("status");
                        }
                        reasoning_items.push(it);
                    }
                    _ => {}
                }
            }
            "response.completed" | "response.incomplete" | "response.done" => {
                completed = true;
                let r = &v["response"];
                served = r["model"].as_str().map(String::from).or(served.take());
                read_usage(&mut usage, &r["usage"]);
                if r["status"] == "incomplete" {
                    stop = "max_tokens".into();
                }
            }
            "response.created" | "response.in_progress" => {
                if let Some(m) = v["response"]["model"].as_str() {
                    served = Some(m.to_string());
                }
            }
            "response.failed" => {
                // What it used before it failed is charged too.
                read_usage(&mut usage, &v["response"]["usage"]);
                let e = &v["response"]["error"];
                let code = e["code"].as_str().unwrap_or_default();
                let class = match code {
                    "rate_limit_exceeded" | "usage_limit_reached" | "insufficient_quota" => ErrorClass::UsageLimit,
                    "context_length_exceeded" | "invalid_prompt" => ErrorClass::InvalidRequest,
                    _ => ErrorClass::Unavailable,
                };
                break Err((class, format!("the response failed: {code} {}", e["message"].as_str().unwrap_or_default())));
            }
            "error" => {
                let code = v["code"].as_str().or_else(|| v["error"]["code"].as_str()).unwrap_or_default().to_string();
                let msg = v["message"].as_str().or_else(|| v["error"]["message"].as_str()).unwrap_or_default().to_string();
                let class = if code.contains("limit") || code.contains("quota") { ErrorClass::UsageLimit } else { ErrorClass::Unavailable };
                break Err((class, format!("stream error {code}: {msg}")));
            }
            _ => {}
        }
    };
    if let Err((c, m)) = r {
        return fail(sink, s, &p.model, &usage, served.as_deref(), c, m).await;
    }
    if !completed {
        return fail(sink, s, &p.model, &usage, served.as_deref(), ErrorClass::Unavailable, "the stream ended before the response completed").await;
    }
    for (id, name, args) in calls {
        let input: Value = if args.trim().is_empty() {
            json!({})
        } else {
            match serde_json::from_str(&args) {
                Ok(v) => v,
                Err(_) => return fail(sink, s, &p.model, &usage, served.as_deref(), ErrorClass::Malformed, format!("tool call {name} has arguments that are not JSON")).await,
            }
        };
        stop = "tool_use".into();
        sink.send(&Event::ToolCall { id, name, input }).await?;
    }
    if !reasoning_items.is_empty() {
        sink.send(&Event::State { state: json!({ "items": reasoning_items }) }).await?;
    }
    finish(sink, s, &p.model, usage, served, &stop).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No prefix is prepended to the instructions: what the provider is
    /// sent is what the kernel logged, with no appliance-config text
    /// outside the version (#269).
    #[test]
    fn no_prefix_is_prepended_to_instructions() {
        let p = CompleteParams {
            model: "m".into(),
            settings: json!({ "instructions_prefix": "SHOULD NOT APPEAR" }),
            credential: None,
            system: "SYS".into(),
            messages: vec![],
            tools: vec![],
            max_tokens: 1,
        };
        assert_eq!(request_body(&p)["instructions"], json!("SYS"));
    }

    fn turn(settings: Value, opening: &str, later: &[&str]) -> CompleteParams {
        let text = |t: &str| magnus_adapter_proto::Message { role: Role::User, content: vec![Block::Text { text: t.into() }], state: None };
        let mut messages = vec![text(opening)];
        messages.extend(later.iter().map(|t| text(t)));
        CompleteParams { model: "m".into(), settings, credential: None, system: "SYS".into(), messages, tools: vec![], max_tokens: 1 }
    }

    /// Every turn of one conversation carries one cache key, so the
    /// provider reads its prefix from cache; another conversation's differs.
    /// Before, no key was sent and 17% of a week's input came from cache.
    #[test]
    fn a_conversations_turns_share_one_cache_key() {
        let key = |p: &CompleteParams| request_body(p)["prompt_cache_key"].as_str().map(String::from);
        let first = key(&turn(json!({}), "Compare the prices.", &[]));
        assert!(first.is_some(), "no cache key was sent");
        assert_eq!(key(&turn(json!({}), "Compare the prices.", &["more", "and more"])), first, "a later turn of the same conversation got another key");
        assert_ne!(key(&turn(json!({}), "Check the inbox.", &[])), first, "two conversations shared a key");
        assert_eq!(key(&turn(json!({ "prompt_cache_key": false }), "Compare the prices.", &[])), None, "a provider set not to take the key was sent one");
    }

    /// The conversation is named in the headers too, by its cache key: the
    /// ChatGPT backend routes a turn to its cache by them.
    #[test]
    fn the_conversation_is_named_in_the_headers() {
        let body = request_body(&turn(json!({}), "Compare the prices.", &[]));
        let k = body["prompt_cache_key"].as_str().unwrap().to_string();
        assert_eq!(conversation_headers(&body), [("session_id", k.clone()), ("x-client-request-id", k)]);
        assert!(conversation_headers(&request_body(&turn(json!({ "prompt_cache_key": false }), "x", &[]))).is_empty());
    }

    /// A reply may ask for several tool calls, so independent reads take one
    /// turn rather than one turn each, each resending the whole context.
    #[test]
    fn a_reply_may_call_several_tools() {
        assert_eq!(request_body(&turn(json!({}), "x", &[]))["parallel_tool_calls"], json!(true));
        assert_eq!(request_body(&turn(json!({ "parallel_tool_calls": false }), "x", &[]))["parallel_tool_calls"], json!(false));
    }
}
