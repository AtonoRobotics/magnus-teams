//! The bridge to programs on this computer (connectors spec, Local desktop
//! connectors, The bridge). While the person is signed in, the app reports
//! the supported programs it finds here and whether each enabled one's
//! server answers, then takes calls on a long poll over its own session to
//! its own server: the only place calls come from, as it listens nowhere.
//! A call names a program, never an address: it goes to the loopback port
//! registered for that program, and only when the person enabled it here.
use crate::detect::{self, Found, LocalPrograms};
use crate::{App, Res};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};

const DEVICE_FILE: &str = "desktop-device.json";
/// How often installs are looked for again.
const DETECT_EVERY: Duration = Duration::from_secs(60);
/// Signed out, or with no server: how long before looking again.
const IDLE_WAIT: Duration = Duration::from_secs(5);
/// A server from before the bridge: asked again only now and then.
const OLD_SERVER_WAIT: Duration = Duration::from_secs(10 * 60);
/// The server's long poll waits 20 seconds; this leaves it room.
const POLL_TIMEOUT: Duration = Duration::from_secs(30);
const REPORT_TIMEOUT: Duration = Duration::from_secs(15);
/// Whether a program's server answers, asked each round.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// Less than the server waits for an answer (two minutes).
const CALL_TIMEOUT: Duration = Duration::from_secs(110);
const MAX_TEXT_CHARS: usize = 512 * 1024;
const MCP_PATH: &str = "/mcp";
const MCP_VERSION: &str = "2025-06-18";

/// The loopback port each program's server answers on. Nothing else is
/// ever reached: a port is registered only on 127.0.0.1, for a supported
/// program, and used only while the person has that program enabled.
#[derive(Default, Clone, Debug)]
pub struct Ports(BTreeMap<String, SocketAddr>);

impl Ports {
    pub fn from_catalog() -> Ports {
        let mut p = Ports::default();
        for prog in crate::programs::PROGRAMS {
            if let Some(port) = prog.mcp_port {
                let _ = p.register(prog.id, SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
            }
        }
        p
    }

    pub fn register(&mut self, program: &str, addr: SocketAddr) -> Res<()> {
        if crate::programs::program(program).is_none() {
            return Err(format!("{program} is not a program the bridge supports"));
        }
        if !addr.ip().is_loopback() || addr.port() == 0 {
            return Err(format!("{addr} is not a port on this computer's loopback, so the bridge will not reach it"));
        }
        self.0.insert(program.to_string(), addr);
        Ok(())
    }

    /// Where a call to `program` goes, if anywhere.
    pub fn target(&self, program: &str, enabled: &BTreeSet<String>) -> Res<SocketAddr> {
        if !enabled.contains(program) {
            return Err(format!("{program} is not enabled on this computer, so nothing was sent to it"));
        }
        let addr = self.0.get(program).ok_or_else(|| format!("{program} has no server registered on this computer, so nothing was sent to it"))?;
        // Checked again where it is used: a registered address is loopback.
        if !addr.ip().is_loopback() {
            return Err(format!("{addr} is not on this computer's loopback"));
        }
        Ok(*addr)
    }
}

/// MCP over HTTP to a program's server on loopback, never through a proxy.
pub struct Mcp {
    http: reqwest::Client,
}

impl Mcp {
    pub fn new() -> Res<Mcp> {
        let http = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().map_err(|e| e.to_string())?;
        Ok(Mcp { http })
    }

    async fn rpc(&self, addr: SocketAddr, session: Option<&str>, body: Value, wait: Duration) -> Res<(Value, Option<String>)> {
        let mut req = self.http.post(format!("http://{addr}{MCP_PATH}")).timeout(wait).header("accept", "application/json, text/event-stream").json(&body);
        if let Some(s) = session {
            req = req.header("mcp-session-id", s);
        }
        let r = req.send().await.map_err(|e| format!("the program's server did not answer: {e}"))?;
        let session = r.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()).map(String::from);
        let sse = r.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|t| t.starts_with("text/event-stream"));
        let status = r.status();
        let text = r.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("the program's server answered {status}"));
        }
        if body.get("id").is_none() {
            return Ok((Value::Null, session));
        }
        Ok((answer_of(&text, sse, &body["id"])?, session))
    }

    /// Open a session: `initialize`, then `notifications/initialized`.
    async fn open(&self, addr: SocketAddr, wait: Duration) -> Res<Option<String>> {
        let init = json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": MCP_VERSION, "capabilities": {}, "clientInfo": { "name": "magnus-bridge", "version": crate::app_version() } } });
        let (_, session) = self.rpc(addr, None, init, wait).await?;
        self.rpc(addr, session.as_deref(), json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }), wait).await?;
        Ok(session)
    }

    /// The program's tools, as its server lists them.
    pub async fn tools(&self, addr: SocketAddr) -> Res<Vec<Value>> {
        let session = self.open(addr, PROBE_TIMEOUT).await?;
        let (v, _) = self.rpc(addr, session.as_deref(), json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }), PROBE_TIMEOUT).await?;
        Ok(v["tools"].as_array().cloned().unwrap_or_default().into_iter().map(|t| json!({ "name": t["name"], "description": t["description"], "input_schema": t["inputSchema"] })).collect())
    }

    /// One call: whether it worked, and its text.
    pub async fn call(&self, addr: SocketAddr, tool: &str, arguments: &Value) -> Res<(bool, String)> {
        let session = self.open(addr, CALL_TIMEOUT).await?;
        let (v, _) = self.rpc(addr, session.as_deref(), json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": tool, "arguments": arguments } }), CALL_TIMEOUT).await?;
        let text: String = v["content"].as_array().map(|c| c.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default();
        Ok((v["isError"] != true, text.chars().take(MAX_TEXT_CHARS).collect()))
    }
}

/// The JSON-RPC result for `id`, from a JSON body or an event stream.
pub fn answer_of(text: &str, sse: bool, id: &Value) -> Res<Value> {
    let messages: Vec<Value> = if sse {
        text.lines().filter_map(|l| l.strip_prefix("data:")).filter_map(|d| serde_json::from_str(d.trim()).ok()).collect()
    } else {
        vec![serde_json::from_str(text).map_err(|_| "the program's server sent something that is not JSON".to_string())?]
    };
    let m = messages.into_iter().find(|m| m["id"] == *id).ok_or("the program's server sent no answer")?;
    match m.get("error") {
        Some(e) => Err(format!("the program's server refused it: {}", e["message"].as_str().unwrap_or("no reason given"))),
        None => Ok(m["result"].clone()),
    }
}

/// Carry out one call from the server, or refuse it.
pub async fn carry_out(job: &Value, ports: &Ports, enabled: &BTreeSet<String>, mcp: &Mcp) -> (bool, String) {
    let (Some(program), Some(tool)) = (job["program"].as_str(), job["tool"].as_str()) else { return (false, "the call named no program or tool".into()) };
    let addr = match ports.target(program, enabled) {
        Ok(a) => a,
        Err(why) => return (false, why),
    };
    let arguments = if job["arguments"].is_object() { job["arguments"].clone() } else { json!({}) };
    match mcp.call(addr, tool, &arguments).await {
        Ok(r) => r,
        Err(why) => (false, format!("{program}: {why}")),
    }
}

/// What this computer reports: each program found, and for an enabled one
/// with a registered port, whether its server answers and its tools.
pub async fn report(found: &[Found], enabled: &BTreeSet<String>, ports: &Ports, mcp: &Mcp) -> Vec<Value> {
    let mut out = vec![];
    for f in found {
        let tools = match ports.target(f.program, enabled) {
            Ok(addr) => mcp.tools(addr).await.ok(),
            Err(_) => None,
        };
        out.push(json!({ "program": f.program, "version": f.version, "path": f.path, "running": tools.is_some(), "tools": tools.unwrap_or_default() }));
    }
    out
}

/// The programs the server says the person enabled on this computer.
pub fn enabled_of(view: &Value) -> BTreeSet<String> {
    view["programs"].as_array().map(|ps| ps.iter().filter(|p| p["allowed"] == true).filter_map(|p| p["program"].as_str().map(String::from)).collect()).unwrap_or_default()
}

/// This computer's id, made here once and kept.
fn device_id() -> Res<String> {
    let dir = dirs::config_dir().ok_or("this computer has no settings folder")?.join("magnus");
    let file = dir.join(DEVICE_FILE);
    if let Some(id) = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v["id"].as_str().map(String::from)) {
        return Ok(id);
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(&file, json!({ "id": id }).to_string()).map_err(|e| e.to_string())?;
    Ok(id)
}

/// Shared with the page: a nudge to report again now (after a consent).
#[derive(Default)]
pub struct Bridge {
    wake: tokio::sync::Notify,
}

/// This computer as the bridge names it, for the page.
#[tauri::command]
pub fn desktop_device() -> Res<Value> {
    Ok(json!({ "id": device_id()?, "name": crate::machine_name() }))
}

/// Report again now, so a program just enabled shows where it stands.
#[tauri::command]
pub fn desktop_refresh(bridge: State<'_, Bridge>) {
    bridge.wake.notify_one();
}

pub fn start(handle: AppHandle) {
    tauri::async_runtime::spawn(run(handle));
}

async fn run(handle: AppHandle) {
    let detector: Arc<dyn LocalPrograms> = Arc::from(detect::system());
    let Ok(mcp) = Mcp::new() else { return };
    let ports = Ports::from_catalog();
    let (mut found, mut detected_at, mut enabled): (Vec<Found>, Option<Instant>, BTreeSet<String>) = (vec![], None, BTreeSet::new());
    loop {
        let session = handle.state::<App>().session();
        let (Ok((url, client, token)), Ok(id)) = (session, device_id()) else {
            tokio::time::sleep(IDLE_WAIT).await;
            continue;
        };
        if detected_at.is_none_or(|t| t.elapsed() >= DETECT_EVERY) {
            let d = detector.clone();
            found = tokio::task::spawn_blocking(move || d.detect()).await.unwrap_or_default();
            detected_at = Some(Instant::now());
        }
        let programs = report(&found, &enabled, &ports, &mcp).await;
        let body = json!({ "id": id, "name": crate::machine_name(), "programs": programs });
        match client.post(format!("{url}/v1/desktop/devices")).bearer_auth(&token).timeout(REPORT_TIMEOUT).json(&body).send().await {
            Ok(r) if r.status().is_success() => enabled = enabled_of(&r.json().await.unwrap_or_default()),
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => {
                tokio::time::sleep(OLD_SERVER_WAIT).await;
                continue;
            }
            _ => {
                tokio::time::sleep(IDLE_WAIT).await;
                continue;
            }
        }
        let bridge = handle.state::<Bridge>().inner();
        let poll = client.get(format!("{url}/v1/desktop/devices/{id}/next")).bearer_auth(&token).timeout(POLL_TIMEOUT).send();
        let job = tokio::select! {
            r = poll => match r {
                Ok(r) if r.status() == reqwest::StatusCode::OK => r.json::<Value>().await.ok(),
                Ok(r) if r.status().is_success() => None,
                _ => {
                    tokio::time::sleep(IDLE_WAIT).await;
                    None
                }
            },
            _ = bridge.wake.notified() => None,
        };
        if let Some(job) = job {
            let (ok, text) = carry_out(&job, &ports, &enabled, &mcp).await;
            let _ = client.post(format!("{url}/v1/desktop/devices/{id}/result")).bearer_auth(&token).timeout(REPORT_TIMEOUT).json(&json!({ "job": job["id"], "ok": ok, "text": text })).send().await;
        }
    }
}

#[cfg(test)]
#[path = "bridge_tests.rs"]
mod tests;
