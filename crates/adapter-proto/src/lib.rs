//! The model adapter contract, `magnus.adapter/1`.
//!
//! An adapter is a separate process that owns everything provider-specific:
//! authentication (API keys, OAuth link flows, token refresh), the model
//! catalog, request and stream translation, error classification and cost.
//! The kernel speaks only this contract.
//!
//! Transport: the adapter listens on a Unix socket given as `--socket <path>`.
//! Each request is one connection: the kernel writes one JSON line
//! `{"contract": "magnus.adapter/1", "method": ..., "params": ...}` and reads
//! JSON event lines until `done`, `error`, or a method's single result.
//! A connection per request lets the kernel move new requests to a replaced
//! adapter without restarting itself.
//!
//! Credentials: the kernel resolves the adapter's credential from the vault and
//! passes it in `credential`. An adapter that refreshes a token returns the
//! new value in a `credential` event; the kernel stores it in the vault. The
//! kernel never logs either.
//!
//! State: each assistant turn may carry an opaque `state` value (signed
//! thinking, encrypted reasoning, a response id). The kernel stores it and
//! sends it back with that turn, and never reads it.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

pub const CONTRACT: &str = "magnus.adapter/1";

#[derive(Serialize, Deserialize, Clone)]
pub struct Envelope {
    pub contract: String,
    #[serde(flatten)]
    pub request: Request,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    /// Identify the adapter. Answered with one `hello` event.
    Hello {},
    /// List models. Answered with one `catalog` event or an `error`.
    Catalog { settings: Value, credential: Option<String> },
    /// Run one model turn. Answered with a stream ending in `done` or `error`.
    Complete(CompleteParams),
    /// Price a call that has not happened, from the adapter's own prices.
    /// Answered with one `quote` event or an `error`. Local: no provider
    /// round trip, no credential needed. The quote scales linearly in
    /// output tokens: an adapter whose pricing tiers quotes the top tier.
    Quote { settings: Value, model: String, input_tokens: u64, output_tokens: u64 },
    /// Measure a prompt as this adapter would send it: what its request
    /// body holds for these messages, in the shared chars-a-token
    /// estimate. Answered with one `prompt_size` event or an `error`.
    /// Local: no provider round trip, no credential needed. An adapter
    /// that drops what it cannot send (a document on a text-only API)
    /// measures what goes out, not what came in.
    EstimatePrompt(EstimateParams),
    /// Start a sign-in. Answered with an `auth_step` event or an `error`.
    AuthBegin { settings: Value },
    /// Finish a sign-in with what the person brought back (callback URL, code).
    /// Answered with `auth_done` carrying the credential to store, or `error`.
    AuthComplete { settings: Value, flow: Value, input: Value },
    /// Revoke a credential at the provider (the kill switch). Answered with
    /// `auth_revoked`, saying whether the provider took it back; an API key
    /// cannot be revoked from here and says so.
    AuthRevoke { settings: Value, credential: String },
    /// Make one picture with the adapter's own sign-in. Answered with one
    /// `image` event or an `error`; a credential refreshed on the way
    /// comes first in a `credential` event. An adapter that cannot make
    /// pictures answers `invalid_request`.
    Image(ImageParams),
}

/// One picture to make, or to change when `source` is given.
#[derive(Serialize, Deserialize, Clone)]
pub struct ImageParams {
    /// The image model: the owner's fast or premium choice in `[images]`.
    pub model: String,
    /// Adapter configuration from the owner, minus the credential name.
    pub settings: Value,
    pub credential: Option<String>,
    pub prompt: String,
    /// The picture to change, for an edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Picture>,
    /// The person asked for the premium picture, not the fast default.
    #[serde(default)]
    pub premium: bool,
}

/// A picture's bytes, base64, with its media type.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Picture {
    pub media_type: String,
    pub data_b64: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CompleteParams {
    pub model: String,
    /// Adapter configuration from the owner, minus the credential name.
    pub settings: Value,
    pub credential: Option<String>,
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: u32,
}

/// A prompt to measure, without asking: the same system, messages and
/// tools a turn would send.
#[derive(Serialize, Deserialize, Clone)]
pub struct EstimateParams {
    pub model: String,
    /// Adapter configuration from the owner, minus the credential name.
    pub settings: Value,
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
}

/// Prompt bytes per token when pricing a call that has not happened.
pub const CHARS_PER_TOKEN: usize = 4;

/// The shared estimate: rendered characters to tokens, rounded up.
pub fn chars_to_tokens(chars: usize) -> u64 {
    chars.div_ceil(CHARS_PER_TOKEN) as u64
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: Vec<Block>,
    /// Opaque provider state from the adapter that produced this assistant turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text { text: String },
    Image { media_type: String, data_b64: String },
    Document { media_type: String, data_b64: String, name: String },
    ToolCall { id: String, name: String, input: Value },
    ToolResult { call_id: String, content: Vec<Block>, is_error: bool },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    /// The provider could not serve the request (connection failure, 5xx,
    /// overload). A retry may help.
    Unavailable,
    /// The provider's response could not be understood.
    Malformed,
    /// A rate, quota or subscription limit. `retry_after_s` says when, if known.
    UsageLimit,
    /// The credential is missing, invalid or expired and cannot be refreshed.
    /// The person must sign in again.
    AuthExpired,
    /// The provider rejected the request itself (for example, context too long).
    InvalidRequest,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    /// Cost of this turn in US dollars, from the adapter's price list.
    pub cost_usd: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    /// True when the id names one fixed model version that will not change.
    pub fixed_version: bool,
    #[serde(default)]
    pub context_window: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Hello {
        contract: String,
        adapter: String,
        version: String,
    },
    Text {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        input: Value,
    },
    Usage(Usage),
    /// The model id the provider reports actually serving this turn.
    ServedModel {
        id: String,
    },
    State {
        state: Value,
    },
    /// A refreshed credential for the vault.
    Credential {
        value: String,
    },
    Done {
        stop: String,
    },
    Error {
        class: ErrorClass,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after_s: Option<f64>,
    },
    Catalog {
        models: Vec<ModelInfo>,
    },
    /// The worst-case dollars for quoted token counts: the prompt at
    /// input prices, the full output at output prices, with no cache
    /// benefit assumed.
    Quote {
        input_usd: f64,
        output_usd: f64,
    },
    /// A prompt's measured tokens, as this adapter would send it.
    PromptSize {
        input_tokens: u64,
    },
    /// What the person must do to sign in: open `url`, or enter `user_code`.
    /// `flow` is opaque and comes back in `auth_complete`.
    AuthStep {
        flow: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user_code: Option<String>,
        instructions: String,
    },
    AuthDone {
        credential: String,
    },
    AuthRevoked {
        at_provider: bool,
        detail: String,
    },
    /// The picture an `image` request made.
    Image(Picture),
}

/// Messages that can carry a credential print as their JSON with every
/// credential replaced: the kernel prints messages it did not expect into
/// errors and tracing, which can reach the log.
fn redacted(m: &impl Serialize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    fn walk(v: &mut Value) {
        match v {
            Value::Object(o) => {
                let is_credential_event = o.get("event").and_then(Value::as_str) == Some("credential");
                for (k, x) in o.iter_mut() {
                    if (k == "credential" || (is_credential_event && k == "value")) && !x.is_null() {
                        *x = Value::String("[redacted]".into());
                    } else {
                        walk(x);
                    }
                }
            }
            Value::Array(a) => a.iter_mut().for_each(walk),
            _ => {}
        }
    }
    let mut v = serde_json::to_value(m).map_err(|_| std::fmt::Error)?;
    walk(&mut v);
    write!(f, "{v}")
}

macro_rules! redacted_debug {
    ($($t:ty),*) => {$(
        impl std::fmt::Debug for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                redacted(self, f)
            }
        }
    )*};
}
redacted_debug!(Envelope, Request, CompleteParams, ImageParams, Event);

/// Send events back to the kernel on one request's connection.
pub struct Sink {
    w: tokio::net::unix::OwnedWriteHalf,
}

impl Sink {
    pub async fn send(&mut self, e: &Event) -> anyhow::Result<()> {
        let mut line = serde_json::to_vec(e)?;
        line.push(b'\n');
        self.w.write_all(&line).await?;
        Ok(())
    }

    pub async fn error(&mut self, class: ErrorClass, message: impl Into<String>) -> anyhow::Result<()> {
        self.send(&Event::Error { class, message: message.into(), retry_after_s: None }).await
    }
}

/// Run an adapter: listen on `socket` and hand each request to `handle`.
pub async fn serve<H, F>(socket: &Path, handle: H) -> anyhow::Result<()>
where
    H: Fn(Request, Sink) -> F + Clone + Send + 'static,
    F: std::future::Future<Output = anyhow::Result<()>> + Send,
{
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket)?;
    loop {
        let (stream, _) = listener.accept().await?;
        let handle = handle.clone();
        tokio::spawn(async move {
            let (r, w) = stream.into_split();
            let mut sink = Sink { w };
            let mut line = String::new();
            if BufReader::new(r).read_line(&mut line).await.is_err() {
                return;
            }
            match serde_json::from_str::<Envelope>(&line) {
                Ok(env) if env.contract == CONTRACT => {
                    if let Err(e) = handle(env.request, sink).await {
                        eprintln!("adapter request failed: {e:#}");
                    }
                }
                Ok(env) => {
                    let _ = sink.error(ErrorClass::InvalidRequest, format!("unsupported contract {}", env.contract)).await;
                }
                Err(e) => {
                    let _ = sink.error(ErrorClass::InvalidRequest, format!("bad request: {e}")).await;
                }
            }
        });
    }
}

/// The socket path from `--socket <path>` on the command line.
pub fn socket_arg() -> anyhow::Result<std::path::PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == "--socket").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from).ok_or_else(|| anyhow::anyhow!("usage: {} --socket <path>", args[0]))
}

/// Kernel side: send one request and receive its events.
pub struct Call {
    lines: tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
    _w: tokio::net::unix::OwnedWriteHalf,
}

impl Call {
    pub async fn open(socket: &Path, request: Request) -> anyhow::Result<Self> {
        let stream = UnixStream::connect(socket).await?;
        let (r, mut w) = stream.into_split();
        let mut line = serde_json::to_vec(&Envelope { contract: CONTRACT.into(), request })?;
        line.push(b'\n');
        w.write_all(&line).await?;
        Ok(Call { lines: BufReader::new(r).lines(), _w: w })
    }

    /// The next event, or `None` when the adapter closed the connection.
    pub async fn next(&mut self) -> anyhow::Result<Option<Event>> {
        match self.lines.next_line().await? {
            Some(l) => Ok(Some(serde_json::from_str(&l)?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod redaction {
    use super::*;

    /// A credential never shows in what the kernel prints of a message:
    /// "unexpected answer: {other:?}" reaches errors, tracing and the log.
    #[test]
    fn printing_a_message_never_shows_its_credential() {
        let secret = "sk-live-0123456789";
        let printed = [
            format!("{:?}", Event::Credential { value: secret.into() }),
            format!("{:?}", Event::AuthDone { credential: secret.into() }),
            format!("{:?}", Request::Catalog { settings: Value::Null, credential: Some(secret.into()) }),
            format!("{:?}", Request::AuthRevoke { settings: Value::Null, credential: secret.into() }),
            format!(
                "{:?}",
                Request::Image(ImageParams { model: "m".into(), settings: Value::Null, credential: Some(secret.into()), prompt: "p".into(), source: None, premium: false })
            ),
            format!(
                "{:?}",
                Envelope {
                    contract: CONTRACT.into(),
                    request: Request::Complete(CompleteParams {
                        model: "m".into(),
                        settings: Value::Null,
                        credential: Some(secret.into()),
                        system: String::new(),
                        messages: vec![],
                        tools: vec![],
                        max_tokens: 1
                    }),
                }
            ),
        ];
        for p in &printed {
            assert!(!p.contains(secret), "a credential was printed: {p}");
        }
        assert!(printed[0].contains("credential") && printed[1].contains("auth_done"), "{printed:?}");
        assert!(format!("{:?}", Event::Text { text: "hello".into() }).contains("hello"));
    }
}
