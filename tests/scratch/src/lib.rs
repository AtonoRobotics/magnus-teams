//! Shared setup for the desktop app's flow tests: an appliance with a fake
//! Anthropic API, a policy that asks before a file leaves a task, a person
//! with a password, and a real Keycloak for sign-in through one (`keycloak.rs`).

use axum::{
    body::Body,
    extract::State,
    http::HeaderMap,
    response::Response,
    routing::{get, post},
    Json, Router,
};
use magnus_invariants::appliance::bin;

pub mod admin;
pub mod brokered;
pub mod catalog;
mod checks;
mod dev_view;
pub use checks::*;
pub use dev_view::*;
mod keycloak;
pub mod mail;
pub mod stand_in;
pub use keycloak::*;
mod oauth_app;
use magnus_invariants::*;
pub use oauth_app::FakeOAuthApp;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;

/// Connect an app as the desktop app does (spec rev 140): read its connect
/// screen and accept it. The screen it accepted.
pub async fn accept_connect_screen(api: &Api, app: &str) -> Value {
    let (st, terms) = api.get(&format!("/v1/connectors/{app}/terms")).await;
    assert!(st.is_success(), "{app}'s connect screen: {terms}");
    let (st, v) = api.post(&format!("/v1/connectors/{app}/consent"), json!({ "terms": terms["terms"] })).await;
    assert!(st.is_success(), "accepting {app}'s connect screen: {v}");
    terms
}

pub const PERSON: &str = "alice";
pub const PASSWORD: &str = "correct horse battery";
pub const CHAT_QUESTION: &str = "What was Q3 revenue?";
pub const CHAT_ANSWER: &str = "Q3 revenue was 41.7M.";
pub const TASK: &str = "Write me a one-line summary of Q3 revenue (41.7M, Q2 was 38.2M) and send me the file.";

/// A scripted reply's text beside its tool calls: the scripted model writes
/// either text or tool calls, so a call to `say` is passed on as text.
pub fn say(text: &str) -> (&'static str, Value) {
    (SAY, json!({ "text": text }))
}

const SAY: &str = "say";

/// In a stream, the pause between a `say` text and the tool calls after it,
/// as a model takes time to write them: longer than a chat view's 2-second
/// refresh, so a view sees the text streaming before anything else.
pub const SAID_PAUSE: std::time::Duration = std::time::Duration::from_secs(3);

/// A model response with each `say` call turned into the text it carries,
/// as one JSON message or as its stream of events, each marked if it comes
/// after a pause.
fn said(body: &str, stream: bool) -> Vec<(bool, String)> {
    let is_say = |b: &Value| b["type"] == "tool_use" && b["name"] == SAY;
    if !stream {
        let Ok(mut msg) = serde_json::from_str::<Value>(body) else { return vec![(false, body.to_string())] };
        let Some(content) = msg["content"].as_array().cloned() else { return vec![(false, body.to_string())] };
        let content: Vec<Value> = content.into_iter().map(|b| if is_say(&b) { json!({ "type": "text", "text": b["input"]["text"] }) } else { b }).collect();
        if !content.iter().any(|b| b["type"] == "tool_use") {
            msg["stop_reason"] = json!("end_turn");
        }
        msg["content"] = json!(content);
        return vec![(false, msg.to_string())];
    }
    let (mut says, mut calls) = (std::collections::HashSet::new(), 0);
    let mut out = vec![];
    for event in body.split("\n\n").filter(|e| !e.trim().is_empty()) {
        let (name, data) = event.split_once("\ndata: ").unwrap_or((event, ""));
        let Ok(mut d) = serde_json::from_str::<Value>(data) else {
            out.push((false, format!("{event}\n\n")));
            continue;
        };
        let mut pause = false;
        match d["type"].as_str() {
            Some("content_block_start") if is_say(&d["content_block"]) => {
                says.insert(d["index"].as_u64());
                d["content_block"] = json!({ "type": "text", "text": "" });
            }
            Some("content_block_start") if d["content_block"]["type"] == "tool_use" => {
                pause = !says.is_empty();
                calls += 1
            }
            Some("content_block_delta") if says.contains(&d["index"].as_u64()) => {
                let input: Value = serde_json::from_str(d["delta"]["partial_json"].as_str().unwrap_or("{}")).unwrap_or_default();
                d["delta"] = json!({ "type": "text_delta", "text": input["text"] });
            }
            Some("message_delta") if calls == 0 && !says.is_empty() => d["delta"]["stop_reason"] = json!("end_turn"),
            _ => {}
        }
        out.push((pause, format!("{name}\ndata: {d}\n\n")));
    }
    out
}

/// `/v1/models` answered here, since connecting a key checks it there;
/// everything else goes to the scripted model.
pub async fn provider(model: &FakeModel) -> String {
    provider_with(model, &["claude-test"]).await
}

/// The same, listing these model ids.
pub async fn provider_with(model: &FakeModel, ids: &[&str]) -> String {
    let data: Vec<Value> = ids.iter().map(|id| json!({ "id": id, "display_name": id, "type": "model" })).collect();
    let listing = json!({ "data": data, "has_more": false });
    #[derive(Clone)]
    struct Up(String, reqwest::Client);
    async fn forward(State(up): State<Up>, headers: HeaderMap, body: String) -> Response {
        let mut req = up.1.post(format!("{}/v1/messages", up.0)).body(body);
        for (k, v) in headers.iter() {
            if k != "host" && k != "content-length" {
                req = req.header(k, v);
            }
        }
        let r = req.send().await.unwrap();
        let mut resp = Response::builder().status(r.status());
        if let Some(ct) = r.headers().get("content-type").cloned() {
            resp = resp.header("content-type", ct);
        }
        let stream = resp.headers_ref().and_then(|h| h.get("content-type")).is_some_and(|c| c.as_bytes().starts_with(b"text/event-stream"));
        let body = String::from_utf8_lossy(&r.bytes().await.unwrap()).to_string();
        let chunks = futures::StreamExt::then(futures::stream::iter(said(&body, stream)), |(pause, chunk)| async move {
            if pause {
                tokio::time::sleep(SAID_PAUSE).await;
            }
            Ok::<_, std::convert::Infallible>(chunk)
        });
        resp.body(Body::from_stream(chunks)).unwrap()
    }
    let app =
        Router::new().route("/v1/models", get(move || async move { Json(listing) })).route("/v1/messages", post(forward)).with_state(Up(model.url.clone(), reqwest::Client::new()));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    url
}

/// A chat turn that starts the task `title` when the person says `ask`, and
/// otherwise says it started. The turn that returns the task's outcome
/// (decisions/2026-11-09-outcomes-return-to-the-agent.md) starts nothing, so
/// a test sees one task.
pub fn starts(ask: &'static str, title: &'static str) -> impl Fn(&str, usize) -> Reply + Send + Sync + 'static {
    move |said, turn| {
        if turn == 0 && said == ask {
            tool("create_task", json!({ "title": title, "goal": format!("{title}.") }))
        } else {
            Reply::Text("Started.".into())
        }
    }
}

/// What a model does for the flow's task: a contract with one criterion
/// checked against the world and one only a person can judge, a plan, a
/// file, its delivery (which asks), and a composed outcome.
pub fn replies(page: String) -> Vec<Reply> {
    let check = json!({ "tool": "web_fetch", "input": { "url": page }, "expect": [{ "pointer": "/text", "contains": "41.7M" }] });
    vec![
        tool(
            "task_contract",
            json!({ "goal": "a one-line revenue summary", "assumptions": [], "irreversible_actions": [],
            "criteria": [
                { "id": "c1", "description": "The finance page still reports Q3 revenue of 41.7M", "check": check },
                { "id": "c2", "description": "The summary reads as one clear line" }
            ] }),
        ),
        tool(
            "task_plan",
            json!({ "units": [{ "id": "u1", "goal": "write the summary", "done_when": "summary.txt exists", "status": "active" }, { "id": "u2", "goal": "deliver it", "done_when": "the person has the file" }] }),
        ),
        tool("sandbox_write_file", json!({ "path": "summary.txt", "content": "Q3 revenue: 41.7M, up from 38.2M in Q2.\n" })),
        tool("deliver_file", json!({ "path": "summary.txt" })),
        tool(
            "task_complete",
            json!({
                "summary": "Wrote the Q3 revenue summary and delivered summary.txt.",
                "deliverable": [{ "type": "file", "file": "summary.txt" }, { "type": "stat", "label": "Q3 revenue", "value": "41.7M" }, { "type": "bar_chart", "title": "Revenue by quarter", "labels": ["Q2", "Q3"], "values": [38.2, 41.7], "unit": "M" }],
                "spoken": "Revenue in the third quarter was forty one point seven million, up from thirty eight point two."
            }),
        ),
    ]
}

pub struct Flow {
    pub app: Appliance,
    pub model: FakeModel,
    pub web: FakeWeb,
}

pub fn set_password(app: &Appliance, person: &str, pw: &str) -> bool {
    let mut c = std::process::Command::new(bin("magnus-appliance"))
        .args(["password", "--config"])
        .arg(app.dir.path().join("magnus.toml"))
        .args(["--person", person])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    writeln!(c.stdin.take().unwrap(), "{pw}").unwrap();
    c.wait().unwrap().success()
}

/// A one-time code for the owner's backup password, as `magnus sign-in-code`
/// prints it on the appliance's machine.
pub fn sign_in_code(app: &Appliance) -> String {
    let out = std::process::Command::new(bin("magnus-appliance")).args(["sign-in-code", "--config"]).arg(app.dir.path().join("magnus.toml")).output().unwrap();
    assert!(out.status.success(), "sign-in-code failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

impl Flow {
    pub async fn start() -> Flow {
        let web = FakeWeb::start(&[("/finance/q3", "<p>Q3 revenue: 41.7M. Q2 revenue: 38.2M.</p>")]).await;
        // Chat turns answer, except that asking for TASK in a conversation
        // starts it as a task (work is started in a conversation); tasks
        // follow the script.
        let task = steps(replies(web.url("/finance/q3")));
        let s = script(move |c| {
            let task = task.clone();
            async move {
                if !c.body["system"].to_string().contains("in a chat") {
                    return task(c).await;
                }
                let first = c.messages().first().cloned().unwrap_or_default();
                let said = first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string();
                match (said == TASK, c.turn()) {
                    (true, 0) => tool("create_task", json!({ "title": "Q3 revenue summary", "goal": TASK })),
                    (true, _) => Reply::Text("Started; it shows here as it goes.".into()),
                    _ => Reply::Text(CHAT_ANSWER.into()),
                }
            }
        });
        Flow::start_with(web, s).await
    }

    /// The same appliance, with the model following `s` instead.
    pub async fn start_with(web: FakeWeb, s: model::Script) -> Flow {
        let mut model = start_checked(s).await;
        model.url = provider(&model).await;
        let app = Appliance::start(Spec::default(), &model).await;
        // Ask before any file leaves a task, so the flow has an approval.
        let ask = "forbid(principal, action == Action::\"execute\", resource == Tool::\"deliver_file\");";
        assert!(app.api(PERSON).put_policy(&format!("user/{PERSON}"), ask).await.is_success());
        assert!(set_password(&app, PERSON, PASSWORD));
        Flow { app, model, web }
    }

    /// Serve HTTPS as well, as a real appliance does. Returns its address and
    /// the certificate fingerprint it printed.
    pub async fn with_tls(&mut self) -> (String, String) {
        // Port 0: the appliance logs the port it bound, which no other test
        // can hold (picking a free port and letting go of it raced).
        let path = self.app.dir.path().join("magnus.toml");
        let mut cfg: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
        cfg["appliance"].as_table_mut().unwrap().insert("tls_listen".into(), toml::Value::String("127.0.0.1:0".into()));
        std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
        self.app.restart().await;
        let (addr, fp) = eventually("the HTTPS address and certificate fingerprint in the log", PATIENCE, || {
            let logs = self.app.logs();
            async move {
                let line = logs.lines().rev().find(|l| l.contains("listening on https://"))?;
                let rest = line.split("listening on https://").nth(1)?;
                Some((rest.split(',').next()?.to_string(), rest.split("certificate SHA-256 ").nth(1)?.trim().to_string()))
            }
        })
        .await;
        // Later restarts keep the port.
        let mut cfg: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
        cfg["appliance"].as_table_mut().unwrap().insert("tls_listen".into(), toml::Value::String(addr.clone()));
        std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
        (format!("https://{addr}"), fp)
    }

    /// What the person saw is what the log says.
    pub async fn check_outcome(&self, key: &str) {
        let alice = self.app.api(PERSON);
        let tasks = alice.tasks().await;
        let id = tasks["tasks"][0]["task_id"].as_str().unwrap().to_string();
        let full = alice.task(&id).await;
        assert_eq!(full["status"], "completed", "{full}");
        assert_eq!(full["criteria"][0]["verified"], true);
        assert_eq!(full["criteria"][0]["verified_by"], "kernel");
        assert_eq!(full["criteria"][1]["verified_by"], format!("person:{PERSON}"));
        let approvals: Vec<Value> = alice.inbox().await.into_iter().filter(|i| i["kind"] == "approval").collect();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0]["resolution"]["by"], format!("person:{PERSON}"), "the approval was not made by the person");
        assert!(!self.app.db.dump().await.contains(key), "the API key reached the log");
        let used = self.model.requests().last().unwrap().headers.get("x-api-key").cloned().unwrap_or_default();
        assert_eq!(used, key, "the appliance did not use the key the person connected");
    }

    /// The task's state and trace, for a failure message.
    pub async fn report(&self) -> String {
        let alice = self.app.api(PERSON);
        let tasks = alice.tasks().await;
        let Some(id) = tasks["tasks"][0]["task_id"].as_str() else { return "--- no task was started ---".into() };
        let (_, tl) = alice.get(&format!("/v1/tasks/{id}/timeline")).await;
        format!("--- task ---\n{}\n--- trace ---\n{}", alice.task(id).await, serde_json::to_string_pretty(&tl).unwrap())
    }
}

/// One HTTP request over the kernel's control socket, as the admin console
/// makes it.
pub async fn control(app: &Appliance, method: &str, path: &str, body: Value) -> (u16, Value) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::UnixStream::connect(app.state_dir.join("control.sock")).await.expect("the kernel's control socket");
    let b = body.to_string();
    s.write_all(format!("{method} {path} HTTP/1.1\r\nHost: kernel\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len()).as_bytes())
        .await
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap();
    (head[9..12].parse().unwrap(), serde_json::from_str(body).unwrap_or(Value::Null))
}

/// Change the appliance's config file, then restart it.
pub async fn reconfigure(app: &mut Appliance, f: impl FnOnce(&mut toml::Table)) {
    let path = app.dir.path().join("magnus.toml");
    let mut cfg: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    f(&mut cfg);
    std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
    app.restart().await;
}

pub fn chromium() -> PathBuf {
    std::env::var("MAGNUS_CHROMIUM")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap()).join(".cache/ms-playwright/chromium-1234/chrome-linux64/chrome"))
}

/// Where a browser check keeps its screenshots, when `MAGNUS_SHOTS` is set:
/// a folder of its own under it, named for the test and numbered for each
/// check the test runs, so checks running at once, or one after another in
/// one test, do not write over each other's evidence (CI run 41279528035:
/// two sign-in flows numbered their screenshots into one folder). Nextest
/// runs each test in its own process, on a thread named for the test.
pub fn shots() -> Option<(&'static str, String)> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let root = std::env::var("MAGNUS_SHOTS").ok()?;
    let test = std::thread::current().name().unwrap_or("test").replace("::", "-");
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    Some(("MAGNUS_SHOTS", format!("{root}/{test}/{n}")))
}
