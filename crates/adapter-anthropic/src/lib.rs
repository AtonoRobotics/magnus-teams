//! Magnus model adapter for the Anthropic Messages API.
//!
//! Settings: `base_url` (default https://api.anthropic.com), optional
//! `anthropic_version`, optional `thinking_budget` (tokens; enables extended
//! thinking), optional `price_per_mtok` {input, output, cache_read, cache_write}
//! overriding the built-in price list. The credential is an API key.
//!
//! Signed thinking blocks travel in the turn's opaque state and are sent back
//! in front of that turn's text and tool calls, as the API requires.

use futures::StreamExt;
use magnus_adapter_proto::{self as proto, Block, ErrorClass, Event, ModelInfo, Request, Role, Sink, Usage};
use serde_json::{json, Value};

const DEFAULT_BASE: &str = "https://api.anthropic.com";
const DEFAULT_VERSION: &str = "2023-06-01";

pub fn main() {
    let socket = match proto::socket_arg() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let http = reqwest::Client::builder().connect_timeout(std::time::Duration::from_secs(10)).build().expect("http client");
    if let Err(e) = rt.block_on(proto::serve(&socket, move |req, sink| handle(http.clone(), req, sink))) {
        eprintln!("adapter stopped: {e:#}");
        std::process::exit(1);
    }
}

async fn handle(http: reqwest::Client, req: Request, mut sink: Sink) -> anyhow::Result<()> {
    match req {
        Request::Hello {} => sink.send(&Event::Hello { contract: proto::CONTRACT.into(), adapter: "anthropic".into(), version: env!("CARGO_PKG_VERSION").into() }).await,
        Request::Catalog { settings, credential } => catalog(&http, &settings, credential, &mut sink).await,
        Request::Complete(p) => complete(&http, p, &mut sink).await,
        Request::Quote { settings, model, input_tokens, output_tokens } => {
            let (pi, po, _, _) = prices(&model, &settings);
            sink.send(&Event::Quote { input_usd: input_tokens as f64 * pi / 1_000_000.0, output_usd: output_tokens as f64 * po / 1_000_000.0 }).await
        }
        Request::EstimatePrompt(p) => {
            // What a turn would send: the same body, measured, not asked.
            let body = request_body(&p.model, &p.settings, &p.system, &p.messages, &p.tools, 0);
            let chars = serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0);
            sink.send(&Event::PromptSize { input_tokens: proto::chars_to_tokens(chars) }).await
        }
        Request::Image(_) => sink.error(ErrorClass::InvalidRequest, "this adapter cannot make images").await,
        Request::AuthRevoke { .. } => {
            let detail = "an API key is revoked in the Anthropic console (https://console.anthropic.com/settings/keys)".into();
            sink.send(&Event::AuthRevoked { at_provider: false, detail }).await
        }
        Request::AuthBegin { .. } => {
            let flow = json!({ "kind": "api_key" });
            sink.send(&Event::AuthStep {
                flow,
                url: Some("https://console.anthropic.com/settings/keys".into()),
                user_code: None,
                instructions: "Create an API key in the Anthropic console and paste it here.".into(),
            })
            .await
        }
        Request::AuthComplete { settings, input, .. } => {
            let key = input["api_key"].as_str().unwrap_or_default().trim().to_string();
            if key.is_empty() {
                return sink.error(ErrorClass::AuthExpired, "no API key given").await;
            }
            // Prove the key works before it is stored.
            let r = http.get(format!("{}/v1/models", base(&settings))).header("x-api-key", &key).header("anthropic-version", version(&settings)).send().await;
            match r {
                Ok(r) if r.status().is_success() => sink.send(&Event::AuthDone { credential: key }).await,
                Ok(r) => {
                    let (class, msg) = classify_status(r.status().as_u16(), &r.text().await.unwrap_or_default());
                    sink.error(class, msg).await
                }
                Err(e) => sink.error(ErrorClass::Unavailable, format!("could not reach Anthropic: {e}")).await,
            }
        }
    }
}

fn base(settings: &Value) -> String {
    settings["base_url"].as_str().unwrap_or(DEFAULT_BASE).trim_end_matches('/').to_string()
}

fn version(settings: &Value) -> String {
    settings["anthropic_version"].as_str().unwrap_or(DEFAULT_VERSION).to_string()
}

async fn catalog(http: &reqwest::Client, settings: &Value, credential: Option<String>, sink: &mut Sink) -> anyhow::Result<()> {
    let Some(key) = credential else {
        return sink.error(ErrorClass::AuthExpired, "no API key in the vault").await;
    };
    let r = http.get(format!("{}/v1/models?limit=100", base(settings))).header("x-api-key", key).header("anthropic-version", version(settings)).send().await;
    let r = match r {
        Ok(r) => r,
        Err(e) => return sink.error(ErrorClass::Unavailable, format!("could not reach Anthropic: {e}")).await,
    };
    let status = r.status().as_u16();
    let text = r.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        let (class, msg) = classify_status(status, &text);
        return sink.error(class, msg).await;
    }
    let v: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return sink.error(ErrorClass::Malformed, "model list is not JSON").await,
    };
    let models = v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str())
        .map(|id| ModelInfo { id: id.into(), fixed_version: !id.ends_with("-latest"), context_window: None })
        .collect();
    sink.send(&Event::Catalog { models }).await
}

/// US dollars per million tokens: (input, output, cache read, cache write).
fn prices(model: &str, settings: &Value) -> (f64, f64, f64, f64) {
    let o = &settings["price_per_mtok"];
    if o.is_object() {
        let g = |k: &str| o[k].as_f64().unwrap_or(0.0);
        return (g("input"), g("output"), g("cache_read"), g("cache_write"));
    }
    // Claude 5.5 list prices (docs/research/providers-and-tac.md).
    let (i, out) = if model.contains("opus") {
        (4.0, 20.0)
    } else if model.contains("haiku") {
        (0.10, 0.50)
    } else {
        (2.0, 10.0)
    };
    (i, out, i * 0.1, i * 1.25)
}

fn to_anthropic(m: &proto::Message) -> Value {
    let mut content: Vec<Value> = vec![];
    if m.role == Role::Assistant {
        if let Some(blocks) = m.state.as_ref().and_then(|s| s["thinking"].as_array()) {
            content.extend(blocks.iter().cloned());
        }
    }
    for b in &m.content {
        content.push(block(b));
    }
    json!({ "role": if m.role == Role::User { "user" } else { "assistant" }, "content": content })
}

fn block(b: &Block) -> Value {
    match b {
        Block::Text { text } => json!({ "type": "text", "text": text }),
        Block::Image { media_type, data_b64 } => {
            json!({ "type": "image", "source": { "type": "base64", "media_type": media_type, "data": data_b64 } })
        }
        Block::Document { media_type, data_b64, name } => json!({
            "type": "document", "title": name,
            "source": { "type": "base64", "media_type": media_type, "data": data_b64 }
        }),
        Block::ToolCall { id, name, input } => json!({ "type": "tool_use", "id": id, "name": name, "input": input }),
        Block::ToolResult { call_id, content, is_error } => json!({
            "type": "tool_result", "tool_use_id": call_id, "is_error": is_error,
            "content": content.iter().map(block).collect::<Vec<_>>()
        }),
    }
}

fn classify_status(status: u16, body: &str) -> (ErrorClass, String) {
    let detail = serde_json::from_str::<Value>(body).ok().and_then(|v| v["error"]["message"].as_str().map(String::from)).unwrap_or_else(|| body.chars().take(300).collect());
    let class = match status {
        401 | 403 => ErrorClass::AuthExpired,
        429 => ErrorClass::UsageLimit,
        400 | 404 | 413 | 422 => ErrorClass::InvalidRequest,
        _ => ErrorClass::Unavailable, // 5xx, 529 overloaded, anything unexpected
    };
    (class, format!("Anthropic returned {status}: {detail}"))
}

/// The request body a turn sends: one builder for the asking and the
/// measuring, so the estimate can never drift from what goes out.
fn request_body(model: &str, settings: &Value, system: &str, messages: &[proto::Message], tools: &[proto::ToolSpec], max_tokens: u32) -> Value {
    let mut body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "system": system,
        "messages": messages.iter().map(to_anthropic).collect::<Vec<_>>(),
        "stream": true,
    });
    if !tools.is_empty() {
        body["tools"] = tools.iter().map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.input_schema })).collect();
    }
    if let Some(budget) = settings["thinking_budget"].as_u64() {
        body["thinking"] = json!({ "type": "enabled", "budget_tokens": budget });
    }
    body
}

async fn complete(http: &reqwest::Client, p: proto::CompleteParams, sink: &mut Sink) -> anyhow::Result<()> {
    let Some(key) = p.credential.clone() else {
        return sink.error(ErrorClass::AuthExpired, "no API key in the vault").await;
    };
    let body = request_body(&p.model, &p.settings, &p.system, &p.messages, &p.tools, p.max_tokens);

    let resp = http
        .post(format!("{}/v1/messages", base(&p.settings)))
        .header("x-api-key", key)
        .header("anthropic-version", version(&p.settings))
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => return sink.error(ErrorClass::Unavailable, format!("could not reach Anthropic: {e}")).await,
    };
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let retry_after = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<f64>().ok());
        let (class, message) = classify_status(status, &resp.text().await.unwrap_or_default());
        return sink.send(&Event::Error { class, message, retry_after_s: retry_after }).await;
    }

    let is_sse = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).map(|v| v.starts_with("text/event-stream")).unwrap_or(false);
    let mut turn = Turn::default();
    if is_sse {
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => return sink.error(ErrorClass::Unavailable, format!("stream broke: {e}")).await,
            };
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(i) = buf.find("\n\n") {
                let frame: String = buf.drain(..i + 2).collect();
                let data: String = frame.lines().filter_map(|l| l.strip_prefix("data:")).map(str::trim).collect::<Vec<_>>().join("");
                if data.is_empty() {
                    continue;
                }
                let Ok(ev) = serde_json::from_str::<Value>(&data) else {
                    return sink.error(ErrorClass::Malformed, "unparseable stream event").await;
                };
                if let Err(msg) = turn.apply(&ev, sink).await? {
                    return sink.error(msg.0, msg.1).await;
                }
            }
        }
    } else {
        let v: Value = match resp.json().await {
            Ok(v) => v,
            Err(_) => return sink.error(ErrorClass::Malformed, "response is not JSON").await,
        };
        turn.from_message(&v, sink).await?;
    }
    turn.finish(&p.model, &p.settings, sink).await
}

#[derive(Default)]
struct Turn {
    served: Option<String>,
    blocks: Vec<Value>,
    partial: Vec<String>,
    usage: (u64, u64, u64, u64),
    stop: Option<String>,
}

type StreamError = (ErrorClass, String);

impl Turn {
    async fn apply(&mut self, ev: &Value, sink: &mut Sink) -> anyhow::Result<Result<(), StreamError>> {
        match ev["type"].as_str().unwrap_or_default() {
            "message_start" => {
                let m = &ev["message"];
                self.served = m["model"].as_str().map(String::from);
                self.add_usage(&m["usage"]);
            }
            "content_block_start" => {
                self.blocks.push(ev["content_block"].clone());
                self.partial.push(String::new());
            }
            "content_block_delta" => {
                let i = ev["index"].as_u64().unwrap_or(0) as usize;
                let d = &ev["delta"];
                let (Some(b), Some(p)) = (self.blocks.get_mut(i), self.partial.get_mut(i)) else {
                    return Ok(Err((ErrorClass::Malformed, "delta for an unknown block".into())));
                };
                match d["type"].as_str().unwrap_or_default() {
                    "text_delta" => {
                        let t = d["text"].as_str().unwrap_or_default();
                        let cur = b["text"].as_str().unwrap_or_default().to_string();
                        b["text"] = json!(cur + t);
                        sink.send(&Event::Text { text: t.into() }).await?;
                    }
                    "input_json_delta" => p.push_str(d["partial_json"].as_str().unwrap_or_default()),
                    "thinking_delta" => {
                        let cur = b["thinking"].as_str().unwrap_or_default().to_string();
                        b["thinking"] = json!(cur + d["thinking"].as_str().unwrap_or_default());
                    }
                    "signature_delta" => b["signature"] = d["signature"].clone(),
                    _ => {}
                }
            }
            "content_block_stop" => {
                let i = ev["index"].as_u64().unwrap_or(0) as usize;
                if let (Some(b), Some(p)) = (self.blocks.get_mut(i), self.partial.get(i)) {
                    if b["type"] == "tool_use" && !p.is_empty() {
                        match serde_json::from_str::<Value>(p) {
                            Ok(input) => b["input"] = input,
                            Err(_) => return Ok(Err((ErrorClass::Malformed, "tool input is not valid JSON".into()))),
                        }
                    }
                }
            }
            "message_delta" => {
                self.stop = ev["delta"]["stop_reason"].as_str().map(String::from);
                self.add_usage(&ev["usage"]);
            }
            "error" => {
                let kind = ev["error"]["type"].as_str().unwrap_or_default();
                let msg = ev["error"]["message"].as_str().unwrap_or_default().to_string();
                let class = match kind {
                    "overloaded_error" | "api_error" => ErrorClass::Unavailable,
                    "rate_limit_error" => ErrorClass::UsageLimit,
                    "authentication_error" | "permission_error" => ErrorClass::AuthExpired,
                    _ => ErrorClass::InvalidRequest,
                };
                return Ok(Err((class, format!("Anthropic stream error {kind}: {msg}"))));
            }
            _ => {}
        }
        Ok(Ok(()))
    }

    async fn from_message(&mut self, m: &Value, sink: &mut Sink) -> anyhow::Result<()> {
        self.served = m["model"].as_str().map(String::from);
        self.add_usage(&m["usage"]);
        self.stop = m["stop_reason"].as_str().map(String::from);
        for b in m["content"].as_array().into_iter().flatten() {
            if b["type"] == "text" {
                sink.send(&Event::Text { text: b["text"].as_str().unwrap_or_default().into() }).await?;
            }
            self.blocks.push(b.clone());
        }
        Ok(())
    }

    fn add_usage(&mut self, u: &Value) {
        let g = |k: &str| u[k].as_u64().unwrap_or(0);
        // Input counts arrive at message_start; output totals arrive at message_delta.
        self.usage.0 = self.usage.0.max(g("input_tokens"));
        self.usage.1 = self.usage.1.max(g("output_tokens"));
        self.usage.2 = self.usage.2.max(g("cache_read_input_tokens"));
        self.usage.3 = self.usage.3.max(g("cache_creation_input_tokens"));
    }

    async fn finish(self, model: &str, settings: &Value, sink: &mut Sink) -> anyhow::Result<()> {
        let mut thinking = vec![];
        for b in &self.blocks {
            match b["type"].as_str().unwrap_or_default() {
                "tool_use" => {
                    sink.send(&Event::ToolCall {
                        id: b["id"].as_str().unwrap_or_default().into(),
                        name: b["name"].as_str().unwrap_or_default().into(),
                        input: if b["input"].is_null() { json!({}) } else { b["input"].clone() },
                    })
                    .await?
                }
                "thinking" | "redacted_thinking" => thinking.push(b.clone()),
                _ => {}
            }
        }
        if !thinking.is_empty() {
            sink.send(&Event::State { state: json!({ "thinking": thinking }) }).await?;
        }
        let (i, o, cr, cw) = self.usage;
        let (pi, po, pcr, pcw) = prices(model, settings);
        let cost = (i as f64 * pi + o as f64 * po + cr as f64 * pcr + cw as f64 * pcw) / 1_000_000.0;
        sink.send(&Event::Usage(Usage { input_tokens: i, output_tokens: o, cache_read_tokens: cr, cache_write_tokens: cw, cost_usd: cost })).await?;
        let Some(served) = self.served else {
            return sink.error(ErrorClass::Malformed, "the response did not name its model").await;
        };
        sink.send(&Event::ServedModel { id: served }).await?;
        sink.send(&Event::Done { stop: self.stop.unwrap_or_else(|| "end_turn".into()) }).await
    }
}
