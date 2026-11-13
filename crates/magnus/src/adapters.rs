//! The kernel side of the model adapter contract (`magnus.adapter/1`).
//!
//! Each adapter is a separate process the kernel starts and supervises. To
//! replace or update one, the kernel starts the new process, checks its
//! contract version, and points new requests at its socket; requests already
//! in flight finish on the old one, which is then stopped. The kernel keeps
//! running throughout.

use crate::config;
use crate::vault::Vault;
use anyhow::{anyhow, Context, Result};
use magnus_adapter_proto::{self as proto, Block, CompleteParams, ErrorClass, Event, Message, Picture, Request, ToolSpec, Usage};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::RwLock;

/// How long a launched adapter has to answer hello, and how often it is tried.
const START_WAIT: Duration = Duration::from_secs(15);
const START_POLL: Duration = Duration::from_millis(30);
/// How long a replaced adapter process lives on to finish what it has in flight.
const RETIRE_AFTER: Duration = Duration::from_secs(600);
/// The longest an adapter's stream may go without an event.
const STREAM_STALL: Duration = Duration::from_secs(600);

pub struct Running {
    pub cfg: config::Adapter,
    pub socket: PathBuf,
    pub adapter: String,
    pub version: String,
    child: Mutex<Child>,
}

impl Drop for Running {
    fn drop(&mut self) {
        let mut c = self.child.lock().unwrap_or_else(|e| e.into_inner());
        let _ = c.kill();
        let _ = c.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

pub struct Adapters {
    dir: PathBuf,
    vault: Arc<Vault>,
    running: RwLock<HashMap<String, Arc<Running>>>,
    configs: RwLock<HashMap<String, config::Adapter>>,
    generation: std::sync::atomic::AtomicU64,
    host: crate::spawn::HostEnv,
}

#[derive(Debug, Clone)]
pub struct AdapterError {
    pub class: ErrorClass,
    pub message: String,
    pub retry_after_s: Option<f64>,
    /// What the call cost, when the provider answered and charged for it
    /// before the call failed.
    pub usage: Option<Usage>,
}

impl AdapterError {
    fn unavailable(m: impl Into<String>) -> Self {
        AdapterError { class: ErrorClass::Unavailable, message: m.into(), retry_after_s: None, usage: None }
    }
}

pub struct TurnOut {
    pub blocks: Vec<Block>,
    pub state: Option<Value>,
    pub usage: Usage,
    pub served_model: String,
    pub stop: String,
}

impl Adapters {
    pub fn new(state_dir: &std::path::Path, vault: Arc<Vault>, cfgs: &[config::Adapter], host: crate::spawn::HostEnv) -> Adapters {
        let dir = state_dir.join("adapters");
        let _ = std::fs::create_dir_all(&dir);
        Adapters {
            dir,
            vault,
            running: RwLock::new(HashMap::new()),
            configs: RwLock::new(cfgs.iter().map(|c| (c.id.clone(), c.clone())).collect()),
            generation: std::sync::atomic::AtomicU64::new(0),
            host,
        }
    }

    pub async fn launch(&self, cfg: &config::Adapter) -> Result<Arc<Running>> {
        anyhow::ensure!(!cfg.command.is_empty(), "adapter {} has no command", cfg.id);
        let gen = self.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let socket = self.dir.join(format!("{}-{}-{gen}.sock", cfg.id, std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let log = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join(format!("{}.log", cfg.id)))?;
        let mut cmd = Command::new(&cfg.command[0]);
        cmd.args(&cfg.command[1..]).arg("--socket").arg(&socket);
        cmd.env_clear().env("PATH", &self.host.path);
        cmd.stdin(Stdio::null()).stdout(Stdio::from(log.try_clone()?)).stderr(Stdio::from(log));
        let child = crate::spawn::spawn(cmd).with_context(|| format!("start adapter {}", cfg.id))?;
        let mut running = Running { cfg: cfg.clone(), socket: socket.clone(), adapter: String::new(), version: String::new(), child: Mutex::new(child) };
        let start = std::time::Instant::now();
        loop {
            if socket.exists() {
                if let Ok(mut call) = proto::Call::open(&socket, Request::Hello {}).await {
                    match call.next().await {
                        Ok(Some(Event::Hello { contract, adapter, version })) => {
                            anyhow::ensure!(contract == proto::CONTRACT, "adapter {} speaks {contract}, not {}", cfg.id, proto::CONTRACT);
                            running.adapter = adapter;
                            running.version = version;
                            return Ok(Arc::new(running));
                        }
                        Ok(other) => anyhow::bail!("adapter {} answered hello with {other:?}", cfg.id),
                        Err(_) => {}
                    }
                }
            }
            if let Ok(Some(st)) = running.child.lock().unwrap_or_else(|e| e.into_inner()).try_wait() {
                anyhow::bail!("adapter {} exited at start ({st})", cfg.id);
            }
            anyhow::ensure!(start.elapsed() < START_WAIT, "adapter {} did not start within {}s", cfg.id, START_WAIT.as_secs());
            tokio::time::sleep(START_POLL).await;
        }
    }

    /// The running process for `id`, started (or restarted) if needed.
    pub async fn get(&self, id: &str) -> Result<Arc<Running>> {
        if let Some(r) = self.running.read().await.get(id) {
            if r.child.lock().unwrap_or_else(|e| e.into_inner()).try_wait().ok().flatten().is_none() {
                return Ok(r.clone());
            }
        }
        let cfg = self.configs.read().await.get(id).cloned().ok_or_else(|| anyhow!("no adapter named {id}"))?;
        let mut w = self.running.write().await;
        if let Some(r) = w.get(id) {
            if r.child.lock().unwrap_or_else(|e| e.into_inner()).try_wait().ok().flatten().is_none() {
                return Ok(r.clone());
            }
        }
        let r = self.launch(&cfg).await?;
        w.insert(id.to_string(), r.clone());
        Ok(r)
    }

    /// Put a launched adapter in service without restarting the kernel. New
    /// requests go to it; the old process finishes what it has in flight.
    pub async fn install(&self, r: Arc<Running>) {
        self.configs.write().await.insert(r.cfg.id.clone(), r.cfg.clone());
        let old = self.running.write().await.insert(r.cfg.id.clone(), r);
        if let Some(old) = old {
            tokio::spawn(async move {
                tokio::time::sleep(RETIRE_AFTER).await;
                drop(old);
            });
        }
    }

    /// Run the conformance suite against a launched adapter.
    pub async fn conformance(&self, r: &Running, model: &str) -> crate::conformance::Report {
        crate::conformance::run(&self.vault, &r.cfg, &r.socket, &r.adapter, &r.version, model).await
    }

    pub async fn list(&self) -> Vec<(String, Vec<String>, Option<(String, String)>)> {
        let cfgs = self.configs.read().await.clone();
        let running = self.running.read().await;
        let mut v: Vec<_> = cfgs.values().map(|c| (c.id.clone(), c.command.clone(), running.get(&c.id).map(|r| (r.adapter.clone(), r.version.clone())))).collect();
        v.sort();
        v
    }

    fn split_settings(cfg: &config::Adapter) -> (Value, Option<String>) {
        let mut s = cfg.settings.clone();
        let cred = s.remove("credential").and_then(|v| v.as_str().map(String::from));
        (Value::Object(s), cred)
    }

    fn credential(&self, name: &Option<String>) -> Option<String> {
        name.as_ref().and_then(|n| self.vault.get(n).ok().flatten()).map(|s| s.expose().to_string())
    }

    fn store_refreshed(&self, name: &Option<String>, value: &str) {
        if let Some(n) = name {
            if let Err(e) = self.vault.put(n, value) {
                tracing::error!("could not store a refreshed credential for {n}: {e}");
            }
        }
    }

    pub async fn complete(
        &self,
        id: &str,
        model: &str,
        system: &str,
        messages: Vec<Message>,
        tools: Vec<ToolSpec>,
        max_tokens: u32,
        mut on_text: impl FnMut(&str),
    ) -> Result<TurnOut, AdapterError> {
        let r = self.get(id).await.map_err(|e| AdapterError::unavailable(format!("adapter {id}: {e:#}")))?;
        let (settings, cred_name) = Self::split_settings(&r.cfg);
        let credential = self.credential(&cred_name);
        let params = CompleteParams { model: model.into(), settings, credential, system: system.into(), messages, tools, max_tokens };
        let mut call = proto::Call::open(&r.socket, Request::Complete(params)).await.map_err(|e| AdapterError::unavailable(format!("adapter {id} unreachable: {e}")))?;
        read_turn(&mut call, id, &mut on_text, |v| self.store_refreshed(&cred_name, v)).await
    }

    /// Start a sign-in for adapter `id`: returns what the person must do.
    pub async fn auth_begin(&self, id: &str) -> Result<Event> {
        let r = self.get(id).await?;
        let (settings, _) = Self::split_settings(&r.cfg);
        let mut call = proto::Call::open(&r.socket, Request::AuthBegin { settings }).await?;
        call.next().await?.ok_or_else(|| anyhow!("adapter closed the connection"))
    }

    /// Finish a sign-in; on success the credential goes straight to the vault.
    pub async fn auth_complete(&self, id: &str, flow: Value, input: Value) -> Result<std::result::Result<(), String>> {
        let r = self.get(id).await?;
        let (settings, cred_name) = Self::split_settings(&r.cfg);
        let name = cred_name.ok_or_else(|| anyhow!("adapter {id} has no credential name in its settings"))?;
        let mut call = proto::Call::open(&r.socket, Request::AuthComplete { settings, flow, input }).await?;
        match call.next().await? {
            Some(Event::AuthDone { credential }) => {
                self.vault.put(&name, &credential)?;
                Ok(Ok(()))
            }
            Some(Event::Error { message, .. }) => Ok(Err(message)),
            other => Ok(Err(format!("unexpected answer: {other:?}"))),
        }
    }

    /// The adapter whose OAuth sign-in is the secret `name`, if any: a
    /// sign-in the provider can revoke (an API key cannot be, from here).
    pub async fn oauth_owner_of(&self, name: &str) -> Option<String> {
        self.configs
            .read()
            .await
            .values()
            .find(|c| {
                let (settings, cred) = Self::split_settings(c);
                cred.as_deref() == Some(name) && settings["auth"]["kind"].as_str().is_some_and(|k| k != "api_key")
            })
            .map(|c| c.id.clone())
    }

    /// Ask adapter `id` to revoke `credential` at its provider (the kill
    /// switch). Returns whether the provider took it back, and what happened.
    pub async fn revoke(&self, id: &str, credential: String) -> (bool, String) {
        let ask = async {
            let r = self.get(id).await?;
            let (settings, _) = Self::split_settings(&r.cfg);
            let mut call = proto::Call::open(&r.socket, Request::AuthRevoke { settings, credential }).await?;
            anyhow::Ok(call.next().await?)
        };
        match ask.await {
            Ok(Some(Event::AuthRevoked { at_provider, detail })) => (at_provider, detail),
            Ok(Some(Event::Error { message, .. })) => (false, message),
            Ok(other) => (false, format!("unexpected answer: {other:?}")),
            Err(e) => (false, format!("{e:#}")),
        }
    }

    /// The prompt's tokens as adapter `id` would send them: what its
    /// request body holds for these messages, not the kernel's bytes. Local
    /// to the adapter: no provider round trip.
    pub async fn estimate_prompt(&self, id: &str, model: &str, system: &str, messages: Vec<Message>, tools: Vec<ToolSpec>) -> Result<u64> {
        let r = self.get(id).await?;
        let (settings, _) = Self::split_settings(&r.cfg);
        let params = proto::EstimateParams { model: model.into(), settings, system: system.into(), messages, tools };
        let mut call = proto::Call::open(&r.socket, Request::EstimatePrompt(params)).await?;
        match call.next().await?.ok_or_else(|| anyhow!("adapter closed the connection"))? {
            Event::PromptSize { input_tokens } => Ok(input_tokens),
            Event::Error { class, message, .. } => Err(anyhow!("adapter {id} would not measure a prompt for {model}: {class:?}: {message}")),
            ev => Err(anyhow!("adapter {id} answered an estimate with {ev:?}")),
        }
    }

    /// Price a call that has not happened, from the adapter's own prices:
    /// the prompt's dollars and the full output's dollars, with no cache
    /// benefit assumed. Local to the adapter: no provider round trip.
    pub async fn quote(&self, id: &str, model: &str, input_tokens: u64, output_tokens: u32) -> Result<(f64, f64)> {
        let r = self.get(id).await?;
        let (settings, _) = Self::split_settings(&r.cfg);
        let mut call = proto::Call::open(&r.socket, Request::Quote { settings, model: model.into(), input_tokens, output_tokens: output_tokens as u64 }).await?;
        match call.next().await?.ok_or_else(|| anyhow!("adapter closed the connection"))? {
            Event::Quote { input_usd, output_usd } => Ok((input_usd, output_usd)),
            Event::Error { class, message, .. } => Err(anyhow!("adapter {id} would not quote {model}: {class:?}: {message}")),
            ev => Err(anyhow!("adapter {id} answered a quote with {ev:?}")),
        }
    }

    /// One picture from adapter `id`, made with its own sign-in. A
    /// credential it refreshed on the way goes to the vault.
    pub async fn image(&self, id: &str, model: &str, prompt: String, source: Option<Picture>, premium: bool) -> Result<Picture> {
        let r = self.get(id).await?;
        let (settings, cred_name) = Self::split_settings(&r.cfg);
        let credential = self.credential(&cred_name);
        let params = proto::ImageParams { model: model.into(), settings, credential, prompt, source, premium };
        let mut call = proto::Call::open(&r.socket, Request::Image(params)).await?;
        loop {
            match call.next().await?.ok_or_else(|| anyhow!("adapter {id} closed the connection"))? {
                Event::Credential { value } => self.store_refreshed(&cred_name, &value),
                Event::Image(p) => return Ok(p),
                Event::Error { class, message, .. } => anyhow::bail!("adapter {id} made no image: {class:?}: {message}"),
                ev => anyhow::bail!("adapter {id} answered an image with {ev:?}"),
            }
        }
    }

    /// The adapter's model list. A credential it refreshed on the way goes
    /// to the vault, never to the caller.
    pub async fn catalog(&self, id: &str) -> Result<Event> {
        let r = self.get(id).await?;
        let (settings, cred_name) = Self::split_settings(&r.cfg);
        let credential = self.credential(&cred_name);
        let mut call = proto::Call::open(&r.socket, Request::Catalog { settings, credential }).await?;
        loop {
            match call.next().await?.ok_or_else(|| anyhow!("adapter closed the connection"))? {
                Event::Credential { value } => self.store_refreshed(&cred_name, &value),
                ev => return Ok(ev),
            }
        }
    }
}

/// Read one turn's events from an adapter: its reply, or the error that
/// ended it with whatever the provider had charged by then.
async fn read_turn(call: &mut proto::Call, id: &str, on_text: &mut impl FnMut(&str), mut store: impl FnMut(&str)) -> Result<TurnOut, AdapterError> {
    let mut text = String::new();
    let mut calls = vec![];
    let (mut state, mut usage, mut served) = (None, None, None);
    loop {
        let ev = tokio::time::timeout(STREAM_STALL, call.next())
            .await
            .map_err(|_| AdapterError { usage: usage.clone(), ..AdapterError::unavailable("adapter stream stalled for 10 minutes") })?
            .map_err(|e| AdapterError { class: ErrorClass::Malformed, message: format!("adapter stream: {e}"), retry_after_s: None, usage: usage.clone() })?;
        match ev {
            None => return Err(AdapterError { usage, ..AdapterError::unavailable(format!("adapter {id} closed the stream early")) }),
            Some(Event::Text { text: t }) => {
                on_text(&t);
                text.push_str(&t);
            }
            Some(Event::ToolCall { id, name, input }) => calls.push(Block::ToolCall { id, name, input }),
            Some(Event::State { state: s }) => state = Some(s),
            Some(Event::Usage(u)) => usage = Some(u),
            Some(Event::ServedModel { id }) => served = Some(id),
            Some(Event::Credential { value }) => store(&value),
            // Charged, then failed: the cost still counts.
            Some(Event::Error { class, message, retry_after_s }) => return Err(AdapterError { class, message, retry_after_s, usage }),
            Some(Event::Done { stop }) => {
                let mut blocks = vec![];
                if !text.is_empty() {
                    blocks.push(Block::Text { text });
                }
                blocks.extend(calls);
                let served_model = served.ok_or_else(|| AdapterError {
                    class: ErrorClass::Malformed,
                    message: "the adapter did not report the served model".into(),
                    retry_after_s: None,
                    usage: usage.clone(),
                })?;
                return Ok(TurnOut {
                    blocks,
                    state,
                    usage: usage.unwrap_or(Usage { input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0, cost_usd: 0.0 }),
                    served_model,
                    stop,
                });
            }
            Some(other) => tracing::warn!("adapter {id} sent an unexpected event: {other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An adapter that reports what the provider charged and then fails
    /// (here, a reply that named no model) hands the cost back with the
    /// error, so the run's budget counts it.
    #[tokio::test]
    async fn a_failed_turn_keeps_what_it_cost() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("a.sock");
        let usage = Usage { input_tokens: 1000, output_tokens: 200, cache_read_tokens: 0, cache_write_tokens: 0, cost_usd: 0.25 };
        let sent = usage.clone();
        let path = socket.clone();
        tokio::spawn(async move {
            proto::serve(&path, move |_, mut sink| {
                let sent = sent.clone();
                async move {
                    sink.send(&Event::Usage(sent)).await?;
                    sink.error(ErrorClass::Malformed, "the response did not name its model").await
                }
            })
            .await
        });
        let params = CompleteParams { model: "m".into(), settings: Value::Null, credential: None, system: String::new(), messages: vec![], tools: vec![], max_tokens: 1 };
        let mut call = loop {
            match proto::Call::open(&socket, Request::Complete(params.clone())).await {
                Ok(c) => break c,
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        };
        let Err(e) = read_turn(&mut call, "a", &mut |_| {}, |_| {}).await else { panic!("the turn did not fail") };
        assert_eq!((e.class, e.usage), (ErrorClass::Malformed, Some(usage)), "the cost was dropped with the error");
    }
}
