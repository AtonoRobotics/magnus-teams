//! Start the appliance as a real process against a fresh database, a fake model
//! and fake apps; talk to it through its HTTP API; kill and restart it.
//!
//! Standard people (the identity provider's view):
//! - alice: groups [staff], roles [owner] — may set policy at every layer, set
//!   secrets, and approve every risk tier.
//! - bob: groups [staff, finance], roles [approver:irreversible,
//!   approver:financial, policy:user] — may approve, and set his own user layer.
//! - carol: groups [staff], no roles.

use crate::{mcp::FakeMcp, model::FakeModel, pg::TestDb, PATIENCE};
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    os::unix::io::AsRawFd,
    path::PathBuf,
    process::Stdio,
    time::Duration,
};
use tokio::process::{Child, Command};

#[derive(Serialize, Clone)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub token: String,
    pub groups: Vec<String>,
    pub roles: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct Tool {
    pub risk: String,
}

#[derive(Serialize, Clone)]
pub struct Connector {
    pub name: String,
    pub transport: String,
    pub url: String,
    pub credential: String,
    /// Who can see what is sent here: private | internal | public.
    pub audience: String,
    /// How results are labeled on the way in: principal | private | untrusted.
    pub label: String,
    /// Identity-provider groups whose members may use this app. Empty: everyone.
    pub access_groups: Vec<String>,
    /// header: the app honors an `Idempotency-Key` header. none: it does not.
    pub idempotency: String,
    pub tools: BTreeMap<String, Tool>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub filter_login_codes: bool,
    /// Where a tool's action goes, read from what it acts on
    /// (`Spec::destinations`).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub destinations: BTreeMap<String, Value>,
    /// How the gate asks whether the person mailed an address before
    /// (`Spec::sent`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent: Option<Value>,
    /// Watched reads (`Spec::watch`): tool answers diffed for new
    /// items that wake a review.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub watch: Vec<Value>,
}

#[derive(Clone)]
pub struct Spec {
    pub people: Vec<Person>,
    pub connectors: Vec<Connector>,
    pub max_model_calls: u32,
    pub max_actions: u32,
    pub max_usd: f64,
    pub model_retries: u32,
    pub verifier: bool,
    /// Connectors whose credential is deliberately not put in the vault.
    pub missing_secrets: Vec<String>,
    /// Extra secrets to store, by name.
    pub secrets: Vec<(String, String)>,
    /// The `[proactive]` config. Off by default, so reviews never wake in
    /// tests that are not about the proactive loop (#237).
    pub proactive: Value,
    /// The `[mail]` config, when the test's appliance sends account email.
    /// None: no mail section, and nothing is sent.
    pub mail: Option<Value>,
    /// Where `magnus-computerd` listens, when the test has computers.
    pub computer: Option<PathBuf>,
    /// More of the `[computer]` section, when a test has computers.
    pub computer_options: Value,
    /// Further top-level config sections.
    pub sections: Value,
}

fn person(id: &str, groups: &[&str], roles: &[&str]) -> Person {
    Person {
        id: id.into(),
        name: id.into(),
        token: format!("tok-{id}-{}", uuid::Uuid::new_v4().simple()),
        groups: groups.iter().map(|s| s.to_string()).collect(),
        roles: roles.iter().map(|s| s.to_string()).collect(),
    }
}

impl Default for Spec {
    fn default() -> Self {
        Spec {
            people: vec![
                person("alice", &["staff"], &["owner"]),
                person("bob", &["staff", "finance"], &["approver:irreversible", "approver:financial", "policy:user"]),
                person("carol", &["staff"], &[]),
            ],
            connectors: vec![],
            max_model_calls: 200,
            max_actions: 200,
            max_usd: 50.0,
            model_retries: 2,
            verifier: false,
            missing_secrets: vec![],
            secrets: vec![],
            proactive: json!({ "on": false }),
            mail: None,
            computer: None,
            computer_options: Value::Null,
            sections: json!({}),
        }
    }
}

impl Spec {
    /// Add an app. `tools` lists (tool, risk class) as the owner classified them.
    pub fn app(mut self, name: &str, mcp: &FakeMcp, audience: &str, label: &str, tools: &[(&str, &str)]) -> Self {
        self.connectors.push(Connector {
            name: name.into(),
            transport: "mcp_http".into(),
            url: mcp.url.clone(),
            credential: format!("{name}_token"),
            audience: audience.into(),
            label: label.into(),
            access_groups: vec![],
            idempotency: "header".into(),
            tools: tools.iter().map(|(t, r)| (t.to_string(), Tool { risk: r.to_string() })).collect(),
            filter_login_codes: false,
            destinations: BTreeMap::new(),
            sent: None,
            watch: vec![],
        });
        self
    }
    /// Has this app's results pass through the login-code filter.
    pub fn filter_login_codes(mut self, app: &str) -> Self {
        self.connectors.iter_mut().find(|c| c.name == app).expect("no such app").filter_login_codes = true;
        self
    }

    /// `tool` of `app` is watched: its answers' items at `items` (a
    /// JSON pointer, `""` for the answer itself) are diffed by their
    /// `id` field, and new ones wake a review for the person (#369).
    pub fn watch(mut self, app: &str, tool: &str, items: &str, id: &str) -> Self {
        let c = self.connectors.iter_mut().find(|c| c.name == app).expect("app");
        c.watch.push(json!({ "tool": tool, "items": items, "id": id }));
        self
    }

    pub fn watch_poll(mut self, app: &str, tool: &str, items: &str, id: &str, poll_minutes: u64) -> Self {
        let c = self.connectors.iter_mut().find(|c| c.name == app).expect("app");
        c.watch.push(json!({ "tool": tool, "items": items, "id": id, "poll_minutes": poll_minutes }));
        self
    }

    pub fn access(mut self, app: &str, groups: &[&str]) -> Self {
        let c = self.connectors.iter_mut().find(|c| c.name == app).expect("app");
        c.access_groups = groups.iter().map(|s| s.to_string()).collect();
        self
    }

    /// `tool` of `app` goes where its `read` tool, called with the action's
    /// `id` field, says: the addresses at `recipients`, with `content`
    /// (JSON pointers into what it returns).
    pub fn destinations(mut self, app: &str, tool: &str, read: &str, id: &str, recipients: &[&str], content: &[&str]) -> Self {
        let c = self.connectors.iter_mut().find(|c| c.name == app).expect("app");
        c.destinations.insert(tool.into(), json!({ "read": read, "id": id, "recipients": recipients, "content": content }));
        self
    }

    /// How the gate asks whether the person mailed an address before: the
    /// `read` tool, called with `input` set to `query` naming the address,
    /// counts as mailed when `found` (a JSON pointer into what it
    /// returns) is not empty.
    pub fn sent(mut self, app: &str, read: &str, input: &str, query: &str, found: &str) -> Self {
        let c = self.connectors.iter_mut().find(|c| c.name == app).expect("app");
        c.sent = Some(json!({ "read": read, "input": input, "query": query, "found": found }));
        self
    }

    pub fn not_idempotent(mut self, app: &str) -> Self {
        let c = self.connectors.iter_mut().find(|c| c.name == app).expect("app");
        c.idempotency = "none".into();
        self
    }

    pub fn without_secret(mut self, app: &str) -> Self {
        self.missing_secrets.push(app.into());
        self
    }

    pub fn max_model_calls(mut self, n: u32) -> Self {
        self.max_model_calls = n;
        self
    }

    pub fn model_retries(mut self, n: u32) -> Self {
        self.model_retries = n;
        self
    }

    pub fn proactive(mut self, v: Value) -> Self {
        self.proactive = v;
        self
    }

    pub fn computer(mut self, socket: &std::path::Path) -> Self {
        self.computer = Some(socket.to_path_buf());
        self
    }

    pub fn section(mut self, name: &str, value: Value) -> Self {
        self.sections[name] = value;
        self
    }
    pub fn computer_options(mut self, options: Value) -> Self {
        self.computer_options = options;
        self
    }

    pub fn mail(mut self, v: Value) -> Self {
        self.mail = Some(v);
        self
    }

    pub fn token(&self, who: &str) -> String {
        self.people.iter().find(|p| p.id == who).expect("person").token.clone()
    }
}

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// Guest kernel and root filesystem for the code sandbox.
pub fn sandbox_assets() -> (PathBuf, PathBuf) {
    let out = workspace_root().join("sandbox/out");
    let k = std::env::var("MAGNUS_GUEST_KERNEL").map(PathBuf::from).unwrap_or(out.join("vmlinux"));
    let r = std::env::var("MAGNUS_GUEST_ROOTFS").map(PathBuf::from).unwrap_or(out.join("rootfs.ext4"));
    (k, r)
}

/// A binary built from this package: cargo puts it beside the `deps` directory
/// that holds the running test executable.
pub fn bin(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().and_then(|d| if d.ends_with("deps") { d.parent() } else { Some(d) }).unwrap();
    let p = dir.join(name);
    assert!(p.exists(), "{} is not built", p.display());
    p
}

/// The address in the appliance's "listening on http://…" line, written
/// after `from` in its log.
fn listening(log: &std::path::Path, from: u64) -> Option<String> {
    let text = std::fs::read(log).ok()?;
    let text = String::from_utf8_lossy(text.get(from as usize..)?);
    let rest = &text[text.find("listening on http://")? + "listening on http://".len()..];
    let host = rest.strip_prefix("127.0.0.1:")?;
    let port: String = host.chars().take_while(char::is_ascii_digit).collect();
    (!port.is_empty()).then(|| format!("127.0.0.1:{port}"))
}

pub struct Appliance {
    pub db: TestDb,
    pub dir: tempfile::TempDir,
    pub state_dir: PathBuf,
    pub spec: Spec,
    pub model: FakeModel,
    /// The model provider's API key, as stored in the vault.
    pub provider_key: String,
    /// App credentials as stored in the vault, by connector name.
    pub app_secrets: HashMap<String, String>,
    pub base: String,
    config_path: PathBuf,
    child: Option<Child>,
    /// Bound before the first start and held until this value is dropped or
    /// `release_port` gives it up, including while the child is dead. A
    /// restart serves on the same socket, so another test's port 0 cannot
    /// be given it in the gap (CI, 2026-11-04: two appliances on
    /// 127.0.0.1:40059). It is a listening socket: a client that connects
    /// while the child is dead is queued here and never answered, so a test
    /// that starts its own process on this port calls `release_port` first.
    listen_socket: Option<std::net::TcpListener>,
    starts: usize,
}

impl Appliance {
    pub async fn start(spec: Spec, model: &FakeModel) -> Self {
        let db = TestDb::create().await;
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("state");
        std::fs::create_dir_all(&state_dir).unwrap();
        let (kernel, rootfs) = sandbox_assets();
        let firecracker = std::env::var("MAGNUS_FIRECRACKER").unwrap_or("/usr/local/bin/firecracker".into());
        // Held for the life of the test. The child inherits a duplicate
        // (MAGNUS_LISTEN_FD) and serves on it; letting the port go between
        // a kill and the next bind is what let another appliance take it.
        let listen_socket = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve a port");
        listen_socket.set_nonblocking(true).expect("nonblocking");
        let listen_at = listen_socket.local_addr().unwrap().to_string();

        let mut config = json!({
            "appliance": { "state_dir": state_dir, "listen": listen_at },
            "database": { "url": db.kernel_url() },
            // The kernel speaks the adapter contract to a separate adapter
            // process; the adapter speaks the provider's own API.
            "model": {
                "adapter": "anthropic",
                "name": "claude-test",
                "max_retries": spec.model_retries
            },
            "adapters": [{
                "id": "anthropic",
                "command": [bin("magnus-adapter-anthropic")],
                "settings": { "base_url": model.url, "credential": "anthropic_api_key" }
            }],
            "budget": {
                "max_model_calls": spec.max_model_calls,
                "max_actions": spec.max_actions,
                "max_usd": spec.max_usd
            },
            "verifier": { "enabled": spec.verifier },
            "web": { "allow_private_networks": true },
            // Openers (#255) run a model turn at a first sign-in and a first
            // app connection; tests that want them turn them on.
            "proposals": { "openers": false },
            "proactive": spec.proactive,
            "sandbox": {
                "firecracker": firecracker,
                "kernel_image": kernel,
                "rootfs": rootfs,
                "vcpus": 1,
                "memory_mib": 512
            },
            "people": spec.people,
            "connectors": spec.connectors,
        });
        if let Some(socket) = &spec.computer {
            config["computer"] = json!({ "socket": socket });
            if let Value::Object(options) = &spec.computer_options {
                for (name, value) in options {
                    config["computer"][name] = value.clone();
                }
            }
        }
        if let Value::Object(sections) = &spec.sections {
            for (name, value) in sections {
                config[name] = value.clone();
            }
        }
        if let Some(mail) = spec.mail.clone() {
            config["mail"] = mail;
        }
        let config_path = dir.path().join("magnus.toml");
        std::fs::write(&config_path, toml::to_string(&config).expect("config as toml")).unwrap();

        let out = std::process::Command::new(bin("magnus-appliance"))
            .args(["migrate", "--config"])
            .arg(&config_path)
            .args(["--admin-url", &db.admin_url()])
            .output()
            .expect("run migrate");
        assert!(out.status.success(), "magnus migrate failed:\n{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));

        // The secrets live in the vault before the first boot: a review
        // or run starting in the first sweep must never meet an empty
        // vault, whatever time it is — outside working hours the
        // overnight review reached the model before the puts below
        // landed, suspended on the sign-in, and held its review slot
        // forever (#445). The API puts still run, storing the same
        // values and recording them in the log.
        let provider_key = crate::canary("provider");
        let mut app_secrets = HashMap::new();
        {
            let vault = magnus::vault::Vault::open(&state_dir).expect("seed the vault");
            vault.put("anthropic_api_key", &provider_key).expect("seed the provider key");
            for c in &spec.connectors {
                if spec.missing_secrets.contains(&c.name) {
                    continue;
                }
                let v = crate::canary(&c.name);
                vault.put(&c.credential, &v).expect("seed the app secret");
                app_secrets.insert(c.name.clone(), v);
            }
            for (n, v) in &spec.secrets {
                vault.put(n, v).expect("seed the extra secret");
            }
        }
        let mut a = Appliance {
            db,
            dir,
            state_dir,
            model: model.clone(),
            provider_key,
            app_secrets,
            // Known once the first start names its port.
            base: String::new(),
            config_path,
            child: None,
            listen_socket: Some(listen_socket),
            starts: 0,
            spec,
        };
        a.spawn().await;

        let owner = a.api("alice");
        assert_eq!(owner.put_secret("anthropic_api_key", &a.provider_key.clone()).await, StatusCode::NO_CONTENT);
        for c in a.spec.connectors.clone() {
            if a.spec.missing_secrets.contains(&c.name) {
                continue;
            }
            let v = crate::canary(&c.name);
            assert_eq!(owner.put_secret(&c.credential, &v).await, StatusCode::NO_CONTENT);
            a.app_secrets.insert(c.name.clone(), v);
        }
        for (n, v) in a.spec.secrets.clone() {
            assert_eq!(owner.put_secret(&n, &v).await, StatusCode::NO_CONTENT);
        }
        a
    }

    async fn spawn(&mut self) {
        self.starts += 1;
        // The log is kept across restarts; this start's lines begin here.
        let from = std::fs::metadata(self.dir.path().join("serve.err")).map(|m| m.len()).unwrap_or(0);
        let log = |kind: &str| std::fs::OpenOptions::new().create(true).append(true).open(self.dir.path().join(format!("serve.{kind}"))).unwrap();
        let raw = self.listen_socket.as_ref().expect("the port was given up").as_raw_fd();
        let mut cmd = Command::new(bin("magnus-appliance"));
        cmd.args(["serve", "--config"]).arg(&self.config_path).env("MAGNUS_LISTEN_FD", "3").stdout(Stdio::from(log("out"))).stderr(Stdio::from(log("err"))).kill_on_drop(true);
        // The child must serve on this process's socket. Binding the port
        // itself fails: this process is already listening, on purpose.
        // SAFETY: dup2 and fcntl are async-signal-safe. `raw` is the listening
        // socket this process holds; the child has not opened fd 3 itself.
        unsafe {
            cmd.pre_exec(move || {
                extern "C" {
                    fn dup2(oldfd: i32, newfd: i32) -> i32;
                    fn fcntl(fd: i32, cmd: i32, arg: i32) -> i32;
                }
                if raw != 3 && dup2(raw, 3) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                // Keep it across exec. The appliance marks it close-on-exec
                // once it is up, and clears that when it execs a new release.
                if fcntl(3, 2, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd.spawn().expect("spawn magnus serve");
        self.child = Some(child);
        let http = reqwest::Client::new();
        let start = std::time::Instant::now();
        loop {
            // Ready only once this start's own log names its port: a health
            // answer alone may come from another test's appliance.
            if let Some(addr) = listening(&self.dir.path().join("serve.err"), from) {
                if self.base.is_empty() {
                    // The socket was reserved before the first start; restarts
                    // inherit it, so the address does not move.
                    self.base = format!("http://{addr}");
                    let mut cfg: toml::Table = toml::from_str(&std::fs::read_to_string(&self.config_path).unwrap()).unwrap();
                    cfg["appliance"].as_table_mut().unwrap().insert("listen".into(), addr.clone().into());
                    std::fs::write(&self.config_path, toml::to_string(&cfg).unwrap()).unwrap();
                }
                assert_eq!(format!("http://{addr}"), self.base, "a restart moved the appliance");
                if let Ok(r) = http.get(format!("{}/v1/health", self.base)).send().await {
                    if r.status().is_success() {
                        return;
                    }
                }
            }
            if let Some(c) = self.child.as_mut() {
                if let Ok(Some(st)) = c.try_wait() {
                    panic!("magnus serve exited with {st}:\n{}", self.logs());
                }
            }
            if start.elapsed() > Duration::from_secs(30) {
                panic!("magnus serve not healthy after 30s:\n{}", self.logs());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// SIGKILL the appliance: no shutdown path runs. The port stays taken
    /// until `release_port` or drop, so a restart cannot lose it.
    pub async fn kill(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.start_kill();
            let _ = c.wait().await;
        }
    }

    /// Stop holding the port. The update test starts its own process on
    /// this address after `kill`; while this socket is still listening that
    /// process cannot bind, and a health check hangs on a connection this
    /// harness accepts into a queue and never reads.
    pub fn release_port(&mut self) {
        self.listen_socket.take();
    }

    pub async fn restart(&mut self) {
        self.kill().await;
        self.spawn().await;
    }

    pub fn api(&self, who: &str) -> Api {
        Api { base: self.base.clone(), token: self.spec.token(who), http: reqwest::Client::new() }
    }

    /// The appliance's own stdout and stderr, across restarts.
    pub fn logs(&self) -> String {
        ["out", "err"].iter().map(|k| std::fs::read_to_string(self.dir.path().join(format!("serve.{k}"))).unwrap_or_default()).collect::<Vec<_>>().join("\n")
    }

    pub fn starts(&self) -> usize {
        self.starts
    }
}

#[derive(Clone)]
pub struct Api {
    pub base: String,
    pub token: String,
    http: reqwest::Client,
}

pub const SETTLED: &[&str] = &["suspended", "completed", "failed", "cancelled"];

impl Api {
    pub async fn send(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = self.http.request(method, format!("{}{}", self.base, path)).bearer_auth(&self.token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        match req.send().await {
            Ok(r) => {
                let st = r.status();
                let text = r.text().await.unwrap_or_default();
                (st, serde_json::from_str(&text).unwrap_or(Value::String(text)))
            }
            Err(e) => panic!("API {path}: {e}"),
        }
    }

    pub async fn get(&self, path: &str) -> (StatusCode, Value) {
        self.send(reqwest::Method::GET, path, None).await
    }

    pub async fn delete(&self, path: &str) -> (StatusCode, Value) {
        self.send(reqwest::Method::DELETE, path, None).await
    }
    pub async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.send(reqwest::Method::POST, path, Some(body)).await
    }

    pub async fn put(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.send(reqwest::Method::PUT, path, Some(body)).await
    }

    /// Hand the worker a task in plain language. Returns the task id.
    pub async fn submit(&self, text: &str) -> String {
        let (st, v) = self.post("/v1/tasks", json!({ "text": text })).await;
        assert!(st.is_success(), "submit task: {st} {v}");
        v["task_id"].as_str().expect("task_id").to_string()
    }

    pub async fn task(&self, id: &str) -> Value {
        let (st, v) = self.get(&format!("/v1/tasks/{id}")).await;
        assert!(st.is_success(), "get task: {st} {v}");
        v
    }

    pub async fn tasks(&self) -> Value {
        let (st, v) = self.get("/v1/tasks").await;
        assert!(st.is_success(), "list tasks: {st} {v}");
        v
    }

    /// Open and resolved items addressed to this person.
    pub async fn inbox(&self) -> Vec<Value> {
        let (st, v) = self.get("/v1/inbox").await;
        assert!(st.is_success(), "inbox: {st} {v}");
        v["items"].as_array().cloned().unwrap_or_default()
    }

    pub async fn respond(&self, item: &str, body: Value) -> StatusCode {
        self.post(&format!("/v1/inbox/{item}"), body).await.0
    }

    /// `layer` is `org`, `group/<id>` or `user/<id>`.
    pub async fn put_policy(&self, layer: &str, cedar: &str) -> StatusCode {
        self.put(&format!("/v1/policies/{layer}"), json!({ "cedar": cedar })).await.0
    }

    pub async fn put_secret(&self, name: &str, value: &str) -> StatusCode {
        self.put(&format!("/v1/secrets/{name}"), json!({ "value": value })).await.0
    }

    pub async fn wait_status(&self, id: &str, wanted: &[&str]) -> Value {
        let me = self.clone();
        crate::eventually(&format!("task {id} reaches one of {wanted:?}"), PATIENCE, || {
            let me = me.clone();
            let id = id.to_string();
            let wanted: Vec<String> = wanted.iter().map(|s| s.to_string()).collect();
            async move {
                let t = me.task(&id).await;
                wanted.iter().any(|w| t["status"] == w.as_str()).then_some(t)
            }
        })
        .await
    }

    pub async fn wait_settled(&self, id: &str) -> Value {
        self.wait_status(id, SETTLED).await
    }

    /// Wait for an open inbox item of `kind` about task `id`.
    pub async fn wait_item(&self, id: &str, kind: &str) -> Value {
        let me = self.clone();
        crate::eventually(&format!("open {kind} item for task {id}"), PATIENCE, || {
            let me = me.clone();
            let (id, kind) = (id.to_string(), kind.to_string());
            async move { me.inbox().await.into_iter().find(|i| i["task_id"] == id.as_str() && i["kind"] == kind.as_str() && i["status"] == "open") }
        })
        .await
    }

    pub async fn open_items(&self, id: &str, kind: &str) -> Vec<Value> {
        self.inbox().await.into_iter().filter(|i| i["task_id"] == id && i["kind"] == kind && i["status"] == "open").collect()
    }
}
