//! Magnus model adapter for OpenAI-compatible providers: GLM, Muse, Codex,
//! Grok and anything else that speaks Chat Completions or Responses.
//!
//! One adapter, configured per provider by settings (see `presets.json`):
//! - `api`: `chat` (POST {base}/chat/completions) or `responses` (POST {base}/responses)
//! - `base_url`
//! - `auth`: `{ "kind": "api_key" }`, `{ "kind": "oauth_device", ... }` or
//!   `{ "kind": "oauth_openai_device", ... }` (see `auth.rs`)
//! - `headers`: extra request headers
//! - `client_version_url`: where the provider publishes its own client's
//!   current version; fills `{client_version}` in `models_path` and `headers`,
//!   read again every `client_version_refresh_s` seconds
//! - `max_tokens_field`: the request field for the output limit, or null to omit
//! - `store`, `include`: Responses options (stateless use: `store: false`,
//!   `include: ["reasoning.encrypted_content"]`)
//! - `send_reasoning`: chat only, send `reasoning_content` back with each turn
//! - `fixed_models`: model ids that name one fixed version
//! - `prices`: { "<model id prefix>": { input, output, cache_read } } in USD per million tokens
//!
//! The credential is an API key, or for OAuth a JSON object with
//! `access_token`, `refresh_token`, `expires_at` (Unix seconds) and optional
//! `account_id`. Tokens near expiry are refreshed and the new credential is
//! returned to the kernel for the vault.

mod auth;
mod chat;
mod responses;

use magnus_adapter_proto::{self as proto, CompleteParams, ErrorClass, Event, ModelInfo, Request, Sink, Usage};
use serde_json::{json, Value};

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

pub(crate) fn base(settings: &Value) -> String {
    settings["base_url"].as_str().unwrap_or("https://api.openai.com/v1").trim_end_matches('/').to_string()
}

/// The token to send, refreshing it first when it is about to expire.
/// Returns (bearer, account id, refreshed credential to store).
pub(crate) async fn bearer(
    http: &reqwest::Client,
    settings: &Value,
    credential: &Option<String>,
    force_refresh: bool,
) -> Result<(String, Option<String>, Option<String>), (ErrorClass, String)> {
    let Some(c) = credential else {
        return Err((ErrorClass::AuthExpired, "no credential in the vault; sign in first".into()));
    };
    match settings["auth"]["kind"].as_str().unwrap_or("api_key") {
        "api_key" => Ok((c.clone(), None, None)),
        _ => auth::oauth_bearer(http, settings, c, force_refresh).await,
    }
}

pub(crate) fn request(http: &reqwest::Client, settings: &Value, path: &str, token: &str, account: &Option<String>) -> reqwest::RequestBuilder {
    let mut r = http.post(format!("{}{path}", base(settings))).bearer_auth(token).header("content-type", "application/json");
    if let (Some(h), Some(a)) = (settings["auth"]["account_header"].as_str(), account) {
        r = r.header(h, a);
    }
    if let Some(hs) = settings["headers"].as_object() {
        for (k, v) in hs {
            if let Some(v) = v.as_str() {
                r = r.header(k, v);
            }
        }
    }
    r
}

/// Refusals that last until the account changes, so asking again cannot
/// help, whatever HTTP status they arrive with. Z.ai sends all of its with
/// 429 (https://docs.z.ai/api-reference/api-code): 1113 no balance, 1309 plan
/// expired, 1311 plan lacks the model, 1314 enterprise plan expired, 1315 key
/// of another product type. Its other 429s are limits that reset. Meta names
/// the type.
const BILLING_REFUSALS: [&str; 6] = ["1113", "1309", "1311", "1314", "1315", "billing_error"];

pub(crate) fn classify(status: u16, body: &str) -> (ErrorClass, String) {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let billing = [&v["error"]["type"], &v["error"]["code"]].iter().any(|k| k.as_str().is_some_and(|k| BILLING_REFUSALS.contains(&k)));
    let detail =
        v["error"]["message"].as_str().or_else(|| v["message"].as_str()).or_else(|| v["detail"].as_str()).map(String::from).unwrap_or_else(|| body.chars().take(300).collect());
    let kind = v["error"]["type"].as_str().or_else(|| v["error"]["code"].as_str()).unwrap_or_default().to_string();
    if billing {
        // InvalidRequest stops at once. A new error class would be logged
        // with each failure, and a kernel rolled back to a release without
        // it could no longer read its log.
        return (ErrorClass::InvalidRequest, format!("billing refused this request, provider returned {status} ({kind}): {detail}"));
    }
    let class = match status {
        401 | 403 => ErrorClass::AuthExpired,
        402 | 429 => ErrorClass::UsageLimit,
        400 | 404 | 413 | 422 => {
            if kind.contains("quota") || kind.contains("usage_limit") || kind.contains("billing") {
                ErrorClass::UsageLimit
            } else {
                ErrorClass::InvalidRequest
            }
        }
        _ => ErrorClass::Unavailable,
    };
    (class, format!("provider returned {status}{}: {detail}", if kind.is_empty() { String::new() } else { format!(" ({kind})") }))
}

pub(crate) fn retry_after(h: &reqwest::header::HeaderMap, body: &str) -> Option<f64> {
    if let Some(s) = h.get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<f64>().ok()) {
        return Some(s);
    }
    let v: Value = serde_json::from_str(body).ok()?;
    let resets = v["error"]["resets_at"].as_i64().or_else(|| v["error"]["resets_in_seconds"].as_i64().map(|s| chrono::Utc::now().timestamp() + s))?;
    Some((resets - chrono::Utc::now().timestamp()).max(0) as f64)
}

/// What a call cost in US dollars, at the model's entry in `prices` (the
/// longest model id prefix that matches). None when the model has no price:
/// it is never counted as free, because a task's budget is the server's
/// per-task limit and a call nobody priced would pass it unseen.
pub(crate) fn cost(settings: &Value, model: &str, u: &Usage) -> Option<f64> {
    let prices = settings["prices"].as_object()?;
    let (_, p) = prices.iter().filter(|(k, _)| model.starts_with(k.as_str())).max_by_key(|(k, _)| k.len())?;
    let g = |k: &str| p[k].as_f64().unwrap_or(0.0);
    let uncached = u.input_tokens.saturating_sub(u.cache_read_tokens) as f64;
    Some((uncached * g("input") + u.cache_read_tokens as f64 * g("cache_read") + u.output_tokens as f64 * g("output")) / 1_000_000.0)
}

/// Why a model cannot be quoted: it has no price in this adapter's settings.
pub(crate) fn unpriced(model: &str) -> String {
    format!("{model} has no price in this adapter's settings, so what it spends cannot be counted against a task's budget: add it under prices (USD per million tokens)")
}

/// `{client_version}` in `models_path` and `headers` becomes the current
/// release of the provider's own client, read from `client_version_url`
/// (plain text, or JSON with `version` as npm serves it). Some backends refuse
/// or filter requests by client version; looking it up keeps the adapter
/// current with no pinned number. Read again after `client_version_refresh_s`;
/// a failed lookup falls back to the last version read.
async fn with_client_version(http: &reqwest::Client, settings: &mut Value) -> Result<(), String> {
    let Some(url) = settings["client_version_url"].as_str().map(String::from) else { return Ok(()) };
    let refresh = settings["client_version_refresh_s"].as_u64().ok_or("client_version_url needs client_version_refresh_s")?;
    let version = client_version(http, &url, refresh).await?;
    if let Some(p) = settings["models_path"].as_str() {
        settings["models_path"] = p.replace("{client_version}", &version).into();
    }
    if let Some(hs) = settings["headers"].as_object_mut() {
        for v in hs.values_mut() {
            if let Some(s) = v.as_str() {
                *v = s.replace("{client_version}", &version).into();
            }
        }
    }
    Ok(())
}

async fn client_version(http: &reqwest::Client, url: &str, refresh_s: u64) -> Result<String, String> {
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};
    static CACHE: OnceLock<Mutex<std::collections::HashMap<String, (String, Instant)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let cached = cache.lock().unwrap().get(url).cloned();
    if let Some((v, at)) = &cached {
        if at.elapsed() < Duration::from_secs(refresh_s) {
            return Ok(v.clone());
        }
    }
    let fetched = async {
        let body = http.get(url).send().await.ok()?.error_for_status().ok()?.text().await.ok()?;
        let v = match serde_json::from_str::<Value>(&body) {
            Ok(j) => j["version"].as_str()?.to_string(),
            Err(_) => body.lines().next()?.trim().to_string(),
        };
        (v.split('.').count() >= 3 && v.starts_with(|c: char| c.is_ascii_digit())).then_some(v)
    }
    .await;
    match (fetched, cached) {
        (Some(v), _) => {
            cache.lock().unwrap().insert(url.to_string(), (v.clone(), Instant::now()));
            Ok(v)
        }
        (None, Some((v, _))) => Ok(v),
        (None, None) => Err(format!("could not read the provider's current client version from {url}")),
    }
}

async fn handle(http: reqwest::Client, mut req: Request, mut sink: Sink) -> anyhow::Result<()> {
    let settings = match &mut req {
        Request::Hello {} | Request::AuthRevoke { .. } | Request::Quote { .. } | Request::EstimatePrompt(..) => None,
        Request::Catalog { settings, .. } | Request::AuthBegin { settings } | Request::AuthComplete { settings, .. } => Some(settings),
        Request::Complete(p) => Some(&mut p.settings),
    };
    if let Some(s) = settings {
        if let Err(m) = with_client_version(&http, s).await {
            return sink.error(ErrorClass::Unavailable, m).await;
        }
    }
    match req {
        Request::Hello {} => sink.send(&Event::Hello { contract: proto::CONTRACT.into(), adapter: "openai-compatible".into(), version: env!("CARGO_PKG_VERSION").into() }).await,
        Request::Catalog { settings, credential } => catalog(&http, &settings, credential, &mut sink).await,
        Request::Complete(p) => match p.settings["api"].as_str().unwrap_or("chat") {
            "responses" => responses::complete(&http, p, &mut sink).await,
            _ => chat::complete(&http, p, &mut sink).await,
        },
        Request::Quote { settings, model, input_tokens, output_tokens } => {
            let input_usd = cost(&settings, &model, &Usage { input_tokens, ..zero_usage() });
            let output_usd = cost(&settings, &model, &Usage { output_tokens, ..zero_usage() });
            match input_usd.zip(output_usd) {
                Some((input_usd, output_usd)) => sink.send(&Event::Quote { input_usd, output_usd }).await,
                None => sink.error(ErrorClass::InvalidRequest, unpriced(&model)).await,
            }
        }
        Request::EstimatePrompt(p) => {
            // What a turn would send: the same body, measured, not asked.
            let full = CompleteParams { model: p.model, settings: p.settings, credential: None, system: p.system, messages: p.messages, tools: p.tools, max_tokens: 0 };
            let body = match full.settings["api"].as_str().unwrap_or("chat") {
                "responses" => responses::request_body(&full),
                _ => chat::request_body(&full),
            };
            let chars = serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0);
            sink.send(&Event::PromptSize { input_tokens: proto::chars_to_tokens(chars) }).await
        }
        Request::AuthBegin { settings } => auth::begin(&http, &settings, &mut sink).await,
        Request::AuthComplete { settings, flow, input } => auth::complete(&http, &settings, &flow, &input, &mut sink).await,
        Request::AuthRevoke { settings, credential } => auth::revoke(&http, &settings, &credential, &mut sink).await,
    }
}

async fn catalog(http: &reqwest::Client, settings: &Value, credential: Option<String>, sink: &mut Sink) -> anyhow::Result<()> {
    let (token, account, refreshed) = match bearer(http, settings, &credential, false).await {
        Ok(t) => t,
        Err((c, m)) => return sink.error(c, m).await,
    };
    if let Some(r) = refreshed {
        sink.send(&Event::Credential { value: r }).await?;
    }
    let path = settings["models_path"].as_str().unwrap_or("/models");
    let mut r = http.get(format!("{}{path}", base(settings))).bearer_auth(&token);
    if let (Some(h), Some(a)) = (settings["auth"]["account_header"].as_str(), &account) {
        r = r.header(h, a);
    }
    if let Some(hs) = settings["headers"].as_object() {
        for (k, v) in hs {
            if let Some(v) = v.as_str() {
                r = r.header(k, v);
            }
        }
    }
    let resp = match r.send().await {
        Ok(r) => r,
        Err(e) => return sink.error(ErrorClass::Unavailable, format!("could not reach the provider: {e}")).await,
    };
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        let (c, m) = classify(status, &body);
        return sink.error(c, m).await;
    }
    let v: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return sink.error(ErrorClass::Malformed, "the model list is not JSON").await,
    };
    let fixed: Vec<String> = settings["fixed_models"].as_array().into_iter().flatten().filter_map(|m| m.as_str().map(String::from)).collect();
    let list = v["data"].as_array().or_else(|| v["models"].as_array()).cloned().unwrap_or_default();
    let models = list
        .iter()
        .filter_map(|m| m["id"].as_str().or_else(|| m["slug"].as_str()).or_else(|| m.as_str()))
        .map(|id| ModelInfo { id: id.into(), fixed_version: fixed.iter().any(|f| f == id), context_window: None })
        .collect();
    sink.send(&Event::Catalog { models }).await
}

/// Emit the end of a turn: usage with cost, the served model, done.
pub(crate) async fn finish(sink: &mut Sink, settings: &Value, model: &str, mut usage: Usage, served: Option<String>, stop: &str) -> anyhow::Result<()> {
    // The served model's price, else the asked one's: the kernel quoted the
    // asked model before the call, so one of them is priced.
    usage.cost_usd = served.as_deref().and_then(|m| cost(settings, m, &usage)).or_else(|| cost(settings, model, &usage)).unwrap_or_default();
    sink.send(&Event::Usage(usage)).await?;
    let Some(served) = served.filter(|s| !s.is_empty()) else {
        return sink.error(ErrorClass::Malformed, "the response did not name its model").await;
    };
    sink.send(&Event::ServedModel { id: served }).await?;
    sink.send(&Event::Done { stop: stop.into() }).await
}

/// Pulls JSON payloads from a server-sent event stream, one at a time, so
/// callers can forward each text delta as it arrives.
pub(crate) struct Sse {
    stream: std::pin::Pin<Box<dyn futures::Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>,
    buf: String,
}

impl Sse {
    pub fn new(resp: reqwest::Response) -> Sse {
        Sse { stream: Box::pin(resp.bytes_stream()), buf: String::new() }
    }

    pub async fn next(&mut self) -> Option<Result<Value, (ErrorClass, String)>> {
        use futures::StreamExt;
        loop {
            while let Some(i) = self.buf.find("\n\n") {
                let frame: String = self.buf.drain(..i + 2).collect();
                let data: String = frame.lines().filter_map(|l| l.strip_prefix("data:")).map(str::trim).collect::<Vec<_>>().join("");
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                return Some(serde_json::from_str::<Value>(&data).map_err(|_| (ErrorClass::Malformed, "unparseable stream event".to_string())));
            }
            match self.stream.next().await {
                Some(Ok(chunk)) => self.buf.push_str(&String::from_utf8_lossy(&chunk).replace("\r\n", "\n")),
                Some(Err(e)) => return Some(Err((ErrorClass::Unavailable, format!("stream broke: {e}")))),
                None => return None,
            }
        }
    }
}

pub(crate) fn zero_usage() -> Usage {
    Usage { input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0, cost_usd: 0.0 }
}

pub(crate) fn tool_specs(tools: &[proto::ToolSpec], responses: bool) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            if responses {
                json!({ "type": "function", "name": t.name, "description": t.description, "parameters": t.input_schema })
            } else {
                json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.input_schema } })
            }
        })
        .collect()
}

#[cfg(test)]
mod billing_tests {
    use super::*;

    /// A model with no price is never counted as free (560 replies were
    /// recorded at $0 on 2026-11-11); one with a price is counted by it.
    #[test]
    fn a_model_without_a_price_has_no_cost() {
        let settings = json!({ "prices": { "gpt-6": { "input": 2.0, "output": 8.0, "cache_read": 0.5 } } });
        let u = Usage { input_tokens: 1_000_000, output_tokens: 1_000_000, ..zero_usage() };
        assert_eq!(cost(&settings, "gpt-6.1-sol", &u), Some(10.0));
        assert_eq!(cost(&settings, "glm-5", &u), None, "an unpriced model counted as free");
        assert_eq!(cost(&json!({}), "gpt-6.1-sol", &u), None, "an adapter with no prices counted as free");
    }

    #[test]
    fn a_billing_refusal_is_not_a_limit_that_passes() {
        let (class, message) = classify(429, r#"{"error":{"code":"1113","message":"Insufficient balance or no resource package. Please recharge."}}"#);
        assert_eq!(class, ErrorClass::InvalidRequest);
        assert!(message.contains("billing") && message.contains("Insufficient balance"), "{message}");
        assert_eq!(classify(402, r#"{"error":{"type":"billing_error","message":"Add a payment method."}}"#).0, ErrorClass::InvalidRequest);
    }

    #[test]
    fn a_limit_that_resets_is_still_a_usage_limit() {
        for code in ["1302", "1308", "1310", "1316"] {
            let body = format!(r#"{{"error":{{"code":"{code}","message":"Limit reached."}}}}"#);
            assert_eq!(classify(429, &body).0, ErrorClass::UsageLimit, "{code}");
        }
    }
}
