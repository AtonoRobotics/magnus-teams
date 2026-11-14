//! Brokered connectors (connectors spec, Brokered API connectors): MCP
//! servers Alpha Vector builds over a provider's public API, run here.
//!
//! - Packaging: a connector's `url` is its server image's digest
//!   (`sha256:<hex>`); the image is an initramfs whose `/init` is the server,
//!   checked against the digest before every boot and booted from the bytes
//!   checked.
//! - Isolation: one instance per person per connector, each its own
//!   Firecracker microVM with no network device and no disk, started at the
//!   person's first call and stopped once unused for `[brokered] idle_s`.
//! - Credentials: the person's current access token (the boundary's
//!   credential stage, refreshed there) goes with each call, never the
//!   refresh token; what comes back is cleaned of it.
//! - Network: the microVM's only way out is vsock to its own egress proxy
//!   here, which admits CONNECT to the hosts its manifest names and nothing
//!   else.
//! - Tools: inputs are checked against the server's schemas before they are
//!   sent and answers after they come back, as the server checks both too.

use crate::apps::{AppTool, CallError, CallOutput, Mcp};
use crate::config::{Brokered as Cfg, Connector, Sandbox};
use anyhow::{anyhow, Context, Result};
use magnus_brokered_kit::{schema, BAD_ANSWER, MCP_PORT, OUTCOME_META, PROTOCOL, PROXY_PORT, ROOTS_ENV, SCOPES_META, TOKEN_META};
use serde_json::{json, Value};
use sha2::Digest;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{unix, TcpStream, UnixListener, UnixStream};

/// A connector's `transport` when it is brokered.
pub const BROKERED: &str = "brokered";
/// As long as the sandbox waits for its guest: an instance boots in about a
/// second, so this only ends one that never will.
const BOOT_WAIT: Duration = Duration::from_secs(30);
/// How often a booting instance is looked at.
const BOOT_POLL: Duration = Duration::from_millis(50);
/// As long as a remote app's call may take (`apps::Mcp`).
const CALL_WAIT: Duration = Duration::from_secs(120);
/// How long the proxy waits to reach a provider host.
const CONNECT_WAIT: Duration = Duration::from_secs(10);
/// The most a CONNECT request's head may be.
const HEAD_MAX: usize = 8192;

/// The image digest a brokered connector's endpoint names: its hex.
pub fn digest(endpoint: &str) -> Result<&str> {
    endpoint
        .strip_prefix("sha256:")
        .filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)))
        .ok_or_else(|| anyhow!("a brokered connector's endpoint is its server image's digest, sha256:<64 hex digits>, not {endpoint:?}"))
}

/// Whether the proxy lets a microVM reach `target` (`host:port`): one of
/// the hosts its manifest names, exactly, where a host alone is its HTTPS
/// port.
pub fn admits(hosts: &[String], target: &str) -> bool {
    let t = target.to_ascii_lowercase();
    hosts.iter().map(|h| h.to_ascii_lowercase()).any(|h| if h.contains(':') { h == t } else { format!("{h}:443") == t })
}

/// The Firecracker configuration of an instance: the sandbox's kernel, the
/// image as its initramfs, no drives and no network device, and vsock.
pub fn vm_config(sb: &Sandbox, cfg: &Cfg, image: &Path, vsock: &Path) -> Value {
    json!({
        "boot-source": { "kernel_image_path": sb.kernel_image, "initrd_path": image, "boot_args": boot_args(cfg) },
        "drives": [],
        // One person's calls, each mostly waiting on the provider.
        "machine-config": { "vcpu_count": 1, "mem_size_mib": cfg.memory_mib },
        "vsock": { "guest_cid": 3, "uds_path": vsock }
    })
}

/// Where a release puts the server images of the catalog's brokered
/// connectors, beside its programs (`release/to-appliance.sh`).
pub const IMAGES: &str = "brokered-images";

/// Where the server images are: `[brokered] images`, or else those of the
/// release the appliance runs.
fn images(cfg: &Cfg, install: Option<&Path>) -> Option<PathBuf> {
    cfg.images.clone().or_else(|| install.map(|d| d.join(IMAGES)).filter(|d| d.is_dir()))
}

/// The kernel's command line, and the roots its server trusts beside the
/// public ones: the kernel hands `magnus_roots` to `/init` as its
/// environment (base64url DER, comma-separated; the kit's `ROOTS_ENV`).
fn boot_args(cfg: &Cfg) -> String {
    use base64::Engine;
    let roots: Vec<String> = cfg.roots.iter().filter_map(|p| std::fs::read(p).ok()).map(|d| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(d)).collect();
    let mut a = "console=ttyS0 reboot=k panic=1 pci=off quiet loglevel=3".to_string();
    if !roots.is_empty() {
        a.push_str(&format!(" {ROOTS_ENV}={}", roots.join(",")));
    }
    a
}

/// One person's instance of one connector (or, with no proxy, an instance
/// that only lists its tools).
struct Instance {
    dir: PathBuf,
    vsock: PathBuf,
    child: Mutex<Option<Child>>,
    proxy: Option<tokio::task::JoinHandle<()>>,
    used: Mutex<Instant>,
    busy: AtomicUsize,
}

impl Instance {
    fn alive(&self) -> bool {
        self.child.lock().unwrap_or_else(|e| e.into_inner()).as_mut().is_some_and(|c| c.try_wait().ok().flatten().is_none())
    }

    /// The microVM, its proxy and its files go.
    fn stop(&self) {
        if let Some(mut c) = self.child.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        if let Some(p) = &self.proxy {
            p.abort();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        self.stop()
    }
}

/// (connector, person)
type Key = (String, String);

/// Whether a call may still go ahead, and why not.
pub type Fence<'a> = dyn Fn() -> std::result::Result<(), String> + Send + Sync + 'a;

pub struct Brokered {
    sandbox: Option<Sandbox>,
    cfg: Cfg,
    root: PathBuf,
    instances: Arc<tokio::sync::Mutex<HashMap<Key, Arc<Instance>>>>,
    /// How many times each scope of `stop` (connector, person, both or
    /// neither) has run: a call holding a token taken before a stop that
    /// covers it sends nothing with it.
    stops: std::sync::Mutex<HashMap<(Option<String>, Option<String>), u64>>,
    /// Each image's tools as its server lists them: an image is known by
    /// its digest, so what it lists is too.
    listed: tokio::sync::Mutex<HashMap<String, Vec<Value>>>,
}

impl Brokered {
    pub fn new(sandbox: Option<Sandbox>, cfg: Cfg, state_dir: &Path) -> Brokered {
        // Short, for the microVMs' socket paths. What a stopped appliance
        // left is no one's now.
        let root = state_dir.join("brk");
        let _ = std::fs::remove_dir_all(&root);
        Brokered { sandbox, cfg, root, instances: Arc::new(tokio::sync::Mutex::new(HashMap::new())), stops: Default::default(), listed: tokio::sync::Mutex::new(HashMap::new()) }
    }

    async fn launch(&self, c: &Connector, hosts: Option<Vec<String>>) -> Result<Instance> {
        let sb = self.sandbox.as_ref().ok_or_else(|| anyhow!("{} runs in a microVM, and this appliance has no [sandbox] to run it", c.name))?;
        let images = images(&self.cfg, crate::update::install_dir().ok().as_deref())
            .ok_or_else(|| anyhow!("{}'s server image is not on this appliance (no {IMAGES} beside its programs, and no [brokered] images)", c.name))?;
        let hex = digest(&c.url)?;
        let bytes = std::fs::read(images.join(format!("{hex}.cpio"))).with_context(|| format!("{}'s server image {hex} is not on this appliance", c.name))?;
        anyhow::ensure!(hex::encode(sha2::Sha256::digest(&bytes)) == hex, "{}'s server image on this server is not the one its manifest names", c.name);
        let dir = self.root.join(&uuid::Uuid::new_v4().simple().to_string()[..12]);
        std::fs::create_dir_all(&dir)?;
        let vsock = dir.join("v.sock");
        let proxy = match hosts {
            // Where Firecracker sends the guest's connections to the host's port.
            Some(h) => Some(tokio::spawn(proxy(UnixListener::bind(dir.join(format!("v.sock_{PROXY_PORT}")))?, h, c.name.clone(), Arc::new(self.cfg.resolve.clone())))),
            None => None,
        };
        let inst = Instance { dir: dir.clone(), vsock: vsock.clone(), child: Mutex::new(None), proxy, used: Mutex::new(Instant::now()), busy: AtomicUsize::new(0) };
        // Booted from the bytes checked, never from a file that could change after.
        let image = dir.join("image.cpio");
        std::fs::write(&image, &bytes)?;
        let config = vm_config(sb, &self.cfg, &image, &vsock);
        anyhow::ensure!(
            config.get("network-interfaces").is_none() && config["drives"].as_array().is_some_and(|d| d.is_empty()),
            "refusing a brokered microVM with a network device or a disk"
        );
        std::fs::write(dir.join("vm.json"), serde_json::to_vec(&config)?)?;
        let console = std::fs::File::create(dir.join("console.log"))?;
        let mut cmd = Command::new(&sb.firecracker);
        cmd.arg("--api-sock").arg(dir.join("api.sock")).arg("--config-file").arg(dir.join("vm.json")).arg("--level").arg("Error");
        cmd.env_clear().stdin(Stdio::null()).stdout(Stdio::from(console.try_clone()?)).stderr(Stdio::from(console));
        *inst.child.lock().unwrap_or_else(|e| e.into_inner()) = Some(crate::spawn::spawn(cmd).context("start firecracker")?);
        let start = Instant::now();
        loop {
            if connect(&vsock).await.is_ok() {
                return Ok(inst);
            }
            if !inst.alive() {
                let log = std::fs::read_to_string(dir.join("console.log")).unwrap_or_default();
                let tail: String = log.chars().rev().take(800).collect::<String>().chars().rev().collect();
                anyhow::bail!("{}'s microVM stopped as it booted: {tail}", c.name);
            }
            if start.elapsed() > BOOT_WAIT {
                anyhow::bail!("{}'s microVM did not come up within {}s", c.name, BOOT_WAIT.as_secs());
            }
            tokio::time::sleep(BOOT_POLL).await;
        }
    }

    /// The image's tools as its server lists them, from an instance of its
    /// own with no way out and no one's token.
    async fn listing(&self, c: &Connector) -> Result<Vec<Value>> {
        let hex = digest(&c.url)?.to_string();
        if let Some(l) = self.listed.lock().await.get(&hex) {
            return Ok(l.clone());
        }
        let inst = self.launch(c, None).await?;
        let r = tokio::time::timeout(CALL_WAIT, async {
            let mut s = Session::open(&inst.vsock).await?;
            s.rpc("tools/list", json!({})).await
        })
        .await
        .map_err(|_| anyhow!("{} did not list its tools in time", c.name))?
        .map_err(|e| anyhow!("{} did not list its tools: {e:?}", c.name))?;
        inst.stop();
        let tools = r["tools"].as_array().cloned().unwrap_or_default();
        self.listed.lock().await.insert(hex, tools.clone());
        Ok(tools)
    }

    pub async fn list_tools(&self, c: &Connector) -> Result<Vec<AppTool>> {
        Ok(self
            .listing(c)
            .await?
            .into_iter()
            .map(|t| AppTool {
                name: t["name"].as_str().unwrap_or_default().into(),
                description: t["description"].as_str().unwrap_or_default().into(),
                input_schema: t.get("inputSchema").cloned().unwrap_or(json!({ "type": "object" })),
                annotations: t.get("annotations").cloned().unwrap_or(Value::Null),
            })
            .collect())
    }

    /// How many stops have covered `person`'s instance of `app`: a call
    /// takes it before its token, and sends nothing if it has changed.
    pub fn generation(&self, app: &str, person: &str) -> u64 {
        let s = self.stops.lock().unwrap_or_else(|e| e.into_inner());
        let (a, p) = (Some(app.to_string()), Some(person.to_string()));
        [(None, None), (a.clone(), None), (None, p.clone()), (a, p)].iter().map(|k| s.get(k).copied().unwrap_or(0)).sum()
    }

    /// The person's instance, running: the one they have, or a new one,
    /// once `fence` says the call may still go ahead.
    async fn instance(&self, c: &Connector, person: &str, fence: &Fence<'_>) -> Result<Arc<Instance>> {
        let key = (c.name.clone(), person.to_string());
        let mut m = self.instances.lock().await;
        // A stop that took the lock first has run by now.
        fence().map_err(|e| anyhow!(e))?;
        if let Some(i) = m.get(&key).filter(|i| i.alive()) {
            *i.used.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
            return Ok(i.clone());
        }
        let i = Arc::new(self.launch(c, Some(c.hosts.clone())).await?);
        m.insert(key.clone(), i.clone());
        tracing::info!("{}: started {person}'s microVM", c.name);
        self.watch(key, Arc::downgrade(&i));
        Ok(i)
    }

    /// Stop an instance once it has gone unused for the idle time.
    fn watch(&self, key: Key, me: Weak<Instance>) {
        let (map, idle) = (self.instances.clone(), Duration::from_secs(self.cfg.idle_s));
        tokio::spawn(async move {
            loop {
                let wait = {
                    let mut m = map.lock().await;
                    let Some(i) = me.upgrade() else { return };
                    if !m.get(&key).is_some_and(|x| Arc::ptr_eq(x, &i)) {
                        return;
                    }
                    let left = idle.saturating_sub(i.used.lock().unwrap_or_else(|e| e.into_inner()).elapsed());
                    if left.is_zero() && i.busy.load(Ordering::SeqCst) == 0 {
                        m.remove(&key);
                        i.stop();
                        tracing::info!("{}: stopped {}'s microVM, unused for {}s", key.0, key.1, idle.as_secs());
                        return;
                    }
                    if left.is_zero() {
                        idle
                    } else {
                        left
                    }
                };
                tokio::time::sleep(wait).await;
            }
        });
    }

    /// Call a tool for `person` with their current access token, unless
    /// `fence` says otherwise once the call has its microVM and again just
    /// before it sends.
    pub async fn call(&self, c: &Connector, person: &str, bearer: Option<&str>, tool: &str, args: &Value, fence: &Fence<'_>) -> Result<CallOutput, CallError> {
        let Some(token) = bearer else { return Err(CallError::NotSent(format!("{} has no sign-in of the person's to call with", c.name))) };
        let listing = self.listing(c).await.map_err(|e| CallError::NotSent(format!("{e:#}")))?;
        let def = listing.iter().find(|t| t["name"] == tool).ok_or_else(|| CallError::NotSent(format!("{} offers no {tool}", c.name)))?;
        if let Err(e) = schema::check(def.get("inputSchema").unwrap_or(&json!({})), args) {
            return Err(CallError::NotSent(format!("{tool}: the input does not match its schema: {e}")));
        }
        let inst = self.instance(c, person, fence).await.map_err(|e| CallError::NotSent(format!("{e:#}")))?;
        inst.busy.fetch_add(1, Ordering::SeqCst);
        let out = tokio::time::timeout(CALL_WAIT, async {
            let mut s = Session::open(&inst.vsock).await?;
            fence().map_err(CallError::NotSent)?;
            s.rpc("tools/call", json!({ "name": tool, "arguments": args, "_meta": { TOKEN_META: token } })).await
        })
        .await
        .unwrap_or_else(|_| Err(CallError::Unknown(format!("{} did not answer within {}s", c.name, CALL_WAIT.as_secs()))));
        *inst.used.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
        inst.busy.fetch_sub(1, Ordering::SeqCst);
        let r = out.map_err(|e| Mcp::clean_err(e, Some(token)))?;
        let text = r["content"].as_array().into_iter().flatten().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n");
        let structured = r.get("structuredContent").cloned().filter(|v| !v.is_null());
        let is_error = r["isError"].as_bool().unwrap_or(false);
        // What the server says happened at the provider, when it failed: a
        // write that may have gone through, a call made whose answer does
        // not match its schema, or a refusal for want of access the
        // connector's scopes name.
        if is_error {
            match r["_meta"][OUTCOME_META].as_str() {
                Some("unknown") => return Err(CallError::Unknown(Mcp::clean(&text, Some(token)))),
                Some(BAD_ANSWER) => return Err(CallError::BadAnswer(Mcp::clean(&text, Some(token)))),
                Some("needs_scope") => {
                    let wanted: Vec<String> =
                        r["_meta"][SCOPES_META].as_array().into_iter().flatten().filter_map(|s| s.as_str()).filter(|s| c.scopes.iter().any(|x| x == s)).map(String::from).collect();
                    if !wanted.is_empty() {
                        return Err(CallError::NeedsScope(wanted));
                    }
                }
                _ => {}
            }
        }
        // Its answer is checked here too: a server with an output schema
        // answers with content that matches it (MCP, Tools). The call was
        // made, so one that does not is a bad answer, never a failure that
        // says nothing happened (#193).
        if let (false, Some(out_schema)) = (is_error, def.get("outputSchema")) {
            let problem = match &structured {
                Some(v) => schema::check(out_schema, v).err(),
                None => Some("it gave no structured content".into()),
            };
            if let Some(e) = problem {
                return Err(CallError::BadAnswer(Mcp::clean(&format!("{tool}: the answer does not match its schema: {e}"), Some(token))));
            }
        }
        Ok(CallOutput { text: Mcp::clean(&text, Some(token)), structured: structured.map(|v| Mcp::clean_value(v, Some(token))), is_error })
    }

    /// Stop the instances of one connector, of one person, of both, or
    /// (with neither) all of them. How many were stopped.
    pub async fn stop(&self, app: Option<&str>, person: Option<&str>) -> usize {
        let mut m = self.instances.lock().await;
        *self.stops.lock().unwrap_or_else(|e| e.into_inner()).entry((app.map(String::from), person.map(String::from))).or_default() += 1;
        let gone: Vec<Key> = m.keys().filter(|(a, p)| app.is_none_or(|x| x == a) && person.is_none_or(|x| x == p)).cloned().collect();
        for k in &gone {
            if let Some(i) = m.remove(k) {
                i.stop();
                tracing::info!("{}: stopped {}'s microVM", k.0, k.1);
            }
        }
        gone.len()
    }

    /// The instances running now: (connector, person).
    pub async fn running(&self) -> Vec<(String, String)> {
        self.instances.lock().await.iter().filter(|(_, i)| i.alive()).map(|(k, _)| k.clone()).collect()
    }
}

/// An MCP session with a server in its microVM, over vsock.
struct Session {
    r: BufReader<unix::OwnedReadHalf>,
    w: unix::OwnedWriteHalf,
    n: u64,
}

async fn connect(vsock: &Path) -> std::io::Result<Session> {
    let (r, mut w) = UnixStream::connect(vsock).await?.into_split();
    // Firecracker's host side of vsock: answered OK once the guest listens.
    w.write_all(format!("CONNECT {MCP_PORT}\n").as_bytes()).await?;
    let mut r = BufReader::new(r);
    let mut ack = String::new();
    r.read_line(&mut ack).await?;
    if !ack.starts_with("OK") {
        return Err(std::io::Error::other("the server is not listening"));
    }
    Ok(Session { r, w, n: 0 })
}

impl Session {
    async fn open(vsock: &Path) -> Result<Session, CallError> {
        let mut s = connect(vsock).await.map_err(|e| CallError::NotSent(format!("could not reach the connector: {e}")))?;
        s.rpc("initialize", json!({ "protocolVersion": PROTOCOL, "capabilities": {}, "clientInfo": { "name": "magnus", "version": env!("CARGO_PKG_VERSION") } })).await?;
        s.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await.map_err(|e| CallError::NotSent(e.to_string()))?;
        Ok(s)
    }

    async fn send(&mut self, msg: &Value) -> std::io::Result<()> {
        self.w.write_all(format!("{msg}\n").as_bytes()).await?;
        self.w.flush().await
    }

    /// A request and its answer. Nothing happened at the provider when the
    /// request was not sent or the server refused it; once sent, what
    /// happened is unknown until it answers.
    async fn rpc(&mut self, method: &str, params: Value) -> Result<Value, CallError> {
        self.n += 1;
        let id = self.n;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await
            .map_err(|e| CallError::NotSent(format!("could not send to the connector: {e}")))?;
        loop {
            let mut line = String::new();
            let n = self.r.read_line(&mut line).await.map_err(|e| CallError::Unknown(format!("the connector's answer was lost: {e}")))?;
            if n == 0 {
                return Err(CallError::Unknown("the connector closed the session before answering".into()));
            }
            let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
            if msg["id"] != json!(id) {
                continue;
            }
            if let Some(e) = msg.get("error") {
                return Err(CallError::NotSent(format!("connector error: {}", e["message"].as_str().unwrap_or("unknown"))));
            }
            return Ok(msg["result"].clone());
        }
    }
}

/// A microVM's egress proxy: each connection from it asks to CONNECT to a
/// host, and only the manifest's hosts are reached.
async fn proxy(l: UnixListener, hosts: Vec<String>, app: String, resolve: Arc<std::collections::BTreeMap<String, String>>) {
    while let Ok((s, _)) = l.accept().await {
        tokio::spawn(tunnel(s, hosts.clone(), app.clone(), resolve.clone()));
    }
}

async fn tunnel(mut s: UnixStream, hosts: Vec<String>, app: String, resolve: Arc<std::collections::BTreeMap<String, String>>) {
    let mut head = vec![];
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match s.read(&mut b).await {
            Ok(1) if head.len() < HEAD_MAX => head.push(b[0]),
            _ => return,
        }
    }
    let line = String::from_utf8_lossy(&head).lines().next().unwrap_or_default().to_string();
    let mut words = line.split_whitespace();
    let (method, target) = (words.next().unwrap_or_default(), words.next().unwrap_or_default().to_string());
    if method != "CONNECT" || !admits(&hosts, &target) {
        tracing::warn!("{app}: the egress proxy refused {line:?}: its manifest names only {}", hosts.join(", "));
        let _ = s.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        return;
    }
    let to = if target.contains(':') { target.to_ascii_lowercase() } else { format!("{}:443", target.to_ascii_lowercase()) };
    let mut up = match tokio::time::timeout(CONNECT_WAIT, dial(&to, &resolve)).await {
        Ok(Ok(u)) => u,
        failed => {
            if let Ok(Err(e)) = failed {
                tracing::warn!("{app}: the egress proxy could not reach {to}: {e:#}");
            }
            let _ = s.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            return;
        }
    };
    if s.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").await.is_ok() {
        let _ = tokio::io::copy_bidirectional(&mut s, &mut up).await;
    }
}

/// Connects to a manifest host (`host:port`): at the address configured for
/// it (`resolve`, an operator's pinned or internal address, or a test's
/// stand-in), or else at an address DNS gives once, refused when it is
/// private, loopback, link-local or unique-local (`web::public_addrs`), so a
/// provider's name pointed inside the server's network reaches nothing
/// there, and the connection goes to the address that was checked.
async fn dial(to: &str, resolve: &std::collections::BTreeMap<String, String>) -> Result<TcpStream> {
    if let Some(addr) = resolve.get(to) {
        return Ok(TcpStream::connect(addr.as_str()).await?);
    }
    let (host, port) = to.rsplit_once(':').ok_or_else(|| anyhow!("{to} names no port"))?;
    let port: u16 = port.parse().map_err(|_| anyhow!("{to} names no port"))?;
    let addrs = crate::web::public_addrs(host, port).await?;
    Ok(TcpStream::connect(&addrs[..]).await?)
}

impl crate::kernel::Kernel {
    /// An app's tools as it lists them now: a remote one with the
    /// credential, a brokered one from its image.
    pub(crate) async fn app_tools(&self, c: &Connector, bearer: Option<&str>) -> Result<Vec<AppTool>> {
        if c.transport == BROKERED {
            return self.brokered.list_tools(c).await;
        }
        self.mcp.list_tools(&c.url, bearer).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A call that has its token and waits for the instances while a stop
    /// covering it takes them first (the kill switch, Disconnect, deletion)
    /// boots nothing and sends nothing once the stop has run. Here the
    /// lock is held, the stop waits on it first and the call after, as the
    /// lock's queue is first come, first served; this appliance has no
    /// sandbox, so a call that went on to boot would say so.
    #[tokio::test(flavor = "current_thread")]
    async fn a_call_waiting_while_its_microvm_is_stopped_boots_nothing() {
        let d = tempfile::tempdir().unwrap();
        let b = Arc::new(Brokered::new(None, Cfg::default(), d.path()));
        let hex = "0a".repeat(32);
        let c: Connector = toml::from_str(&format!(
            "name = \"mail\"\ntransport = \"brokered\"\nurl = \"sha256:{hex}\"\naudience = \"private\"\nlabel = \"private+untrusted\"\nhosts = [\"api.example\"]"
        ))
        .unwrap();
        b.listed.lock().await.insert(hex, vec![json!({ "name": "fetch", "inputSchema": { "type": "object" } })]);
        // Another person's microVM is booting.
        let booting = b.instances.lock().await;
        // Alice's call took her token (and the count of stops) first.
        let stops = b.generation("mail", "alice");
        let (b1, b2) = (b.clone(), b.clone());
        let stop = tokio::spawn(async move { b1.stop(Some("mail"), Some("alice")).await });
        tokio::task::yield_now().await;
        let call = tokio::spawn(async move {
            let fence = || if b2.generation("mail", "alice") == stops { Ok(()) } else { Err("stopped while this call waited".to_string()) };
            b2.call(&c, "alice", Some("tok"), "fetch", &json!({}), &fence).await
        });
        tokio::task::yield_now().await;
        drop(booting);
        stop.await.unwrap();
        match call.await.unwrap() {
            Err(CallError::NotSent(e)) => assert!(e.contains("stopped while this call waited") && !e.contains("[sandbox]"), "{e}"),
            other => panic!("{:?}", other.err()),
        }
        assert!(b.instances.lock().await.is_empty());
        // Another person's calls are not fenced by her stop.
        assert_eq!(b.generation("mail", "bob"), 0);
        assert_eq!(b.generation("mail", "alice"), 1);
        b.stop(None, None).await;
        assert_eq!((b.generation("mail", "bob"), b.generation("drive", "alice"), b.generation("mail", "alice")), (1, 1, 2));
    }

    #[test]
    fn the_proxy_admits_only_the_manifests_hosts_exactly() {
        let hosts = vec!["gmail.googleapis.com".to_string(), "127.0.0.1:8080".to_string()];
        assert!(admits(&hosts, "gmail.googleapis.com:443"));
        assert!(admits(&hosts, "GMAIL.googleapis.com:443"));
        assert!(admits(&hosts, "127.0.0.1:8080"));
        for t in ["gmail.googleapis.com:80", "googleapis.com:443", "evil.gmail.googleapis.com:443", "gmail.googleapis.com.evil:443", "127.0.0.1:8081", "127.0.0.1:443", ""] {
            assert!(!admits(&hosts, t), "{t}");
        }
    }

    /// What the egress proxy answers a microVM's CONNECT to `target`.
    async fn proxy_answer(target: &str, hosts: &[&str], resolve: &[(&str, String)]) -> String {
        let (mut vm, host_side) = UnixStream::pair().unwrap();
        let hosts = hosts.iter().map(|h| h.to_string()).collect();
        let resolve = Arc::new(resolve.iter().map(|(k, v)| (k.to_string(), v.clone())).collect());
        let t = tokio::spawn(tunnel(host_side, hosts, "mail".into(), resolve));
        vm.write_all(format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n").as_bytes()).await.unwrap();
        let mut line = String::new();
        BufReader::new(&mut vm).read_line(&mut line).await.unwrap();
        drop(vm);
        t.await.unwrap();
        line
    }

    /// A manifest host whose name DNS points inside the server's network
    /// (here `localhost`, or an address literal) is refused at the proxy,
    /// even with a listener there; one the config pins to an internal
    /// address (a stand-in, an operator's own server) is reached.
    #[tokio::test(flavor = "current_thread")]
    async fn the_proxy_refuses_a_private_address_unless_configured() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((s, _)) = l.accept().await {
                drop(s);
            }
        });
        for target in [format!("localhost:{port}"), format!("127.0.0.1:{port}"), format!("[::1]:{port}"), format!("[::ffff:127.0.0.1]:{port}")] {
            let got = proxy_answer(&target, &[&target], &[]).await;
            assert!(got.starts_with("HTTP/1.1 502"), "{target}: {got}");
        }
        let target = format!("api.example:{port}");
        let got = proxy_answer(&target, &[&target], &[(&target, format!("127.0.0.1:{port}"))]).await;
        assert!(got.starts_with("HTTP/1.1 200"), "{got}");
        assert!(dial("api.example", &Default::default()).await.is_err(), "a target with no port");
    }

    #[test]
    fn an_instance_has_no_network_device_and_no_disk() {
        let sb = Sandbox { firecracker: "/fc".into(), kernel_image: "/k".into(), rootfs: "/r".into(), jailer: None, vcpus: 1, memory_mib: 512, workspace_mib: 1 };
        let c = vm_config(&sb, &Cfg::default(), Path::new("/i.cpio"), Path::new("/v.sock"));
        assert!(c.get("network-interfaces").is_none());
        assert_eq!(c["drives"], json!([]));
        assert_eq!(c["boot-source"]["initrd_path"], "/i.cpio");
    }

    /// The images are the release's unless the config names others.
    #[test]
    fn a_release_brings_its_server_images() {
        let install = tempfile::tempdir().unwrap();
        assert_eq!(images(&Cfg::default(), Some(install.path())), None);
        std::fs::create_dir(install.path().join(IMAGES)).unwrap();
        assert_eq!(images(&Cfg::default(), Some(install.path())), Some(install.path().join(IMAGES)));
        let cfg = Cfg { images: Some("/elsewhere".into()), ..Cfg::default() };
        assert_eq!(images(&cfg, Some(install.path())), Some(PathBuf::from("/elsewhere")));
    }

    #[test]
    fn an_image_is_named_by_its_full_digest() {
        assert!(digest(&format!("sha256:{}", "0a".repeat(32))).is_ok());
        for e in ["https://a.example/mcp", "sha256:abc", &format!("sha256:{}", "0A".repeat(32)), &format!("sha512:{}", "0a".repeat(32))] {
            assert!(digest(e).is_err(), "{e}");
        }
    }
}
