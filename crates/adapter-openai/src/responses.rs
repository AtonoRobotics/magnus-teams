//! Responses (`POST {base}/responses`, streamed), used statelessly: `store`
//! false and the full input every turn. Opaque state per assistant turn:
//! `{ "items": [...] }`, the turn's reasoning items (with encrypted content),
//! sent back in front of that turn's output.

use crate::{finish, post_signed_in, tool_specs, zero_usage, Sse};
use magnus_adapter_proto::{Block, CompleteParams, ErrorClass, Event, Role, Sink};
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

/// The request body a turn sends: one builder for the asking and the
/// measuring, so the estimate can never drift from what goes out.
pub fn request_body(p: &CompleteParams) -> Value {
    let s = &p.settings;
    // No prefix: what the provider is sent is what the kernel logged, with
    // no appliance-config text outside the version (#269).
    let mut body = json!({
        "model": p.model,
        "instructions": p.system,
        "input": input(p),
        "stream": true,
        "store": s["store"].as_bool().unwrap_or(false),
        "tool_choice": "auto",
        "parallel_tool_calls": false,
    });
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
    if let Some(extra) = s["body"].as_object() {
        for (k, v) in extra {
            body[k.as_str()] = v.clone();
        }
    }
    body
}

pub async fn complete(http: &reqwest::Client, p: CompleteParams, sink: &mut Sink) -> anyhow::Result<()> {
    let s = &p.settings;
    let body = request_body(&p);
    let Some(resp) = post_signed_in(http, s, &p.credential, "/responses", &body, |r| r.header("accept", "text/event-stream"), sink).await? else {
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
                let u = &r["usage"];
                usage.input_tokens = u["input_tokens"].as_u64().unwrap_or(0);
                usage.output_tokens = u["output_tokens"].as_u64().unwrap_or(0);
                usage.cache_read_tokens = u["input_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0);
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
        return sink.error(c, m).await;
    }
    if !completed {
        return sink.error(ErrorClass::Unavailable, "the stream ended before the response completed").await;
    }
    for (id, name, args) in calls {
        let input: Value = if args.trim().is_empty() {
            json!({})
        } else {
            match serde_json::from_str(&args) {
                Ok(v) => v,
                Err(_) => return sink.error(ErrorClass::Malformed, format!("tool call {name} has arguments that are not JSON")).await,
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
}
