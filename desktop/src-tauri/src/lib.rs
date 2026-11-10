//! The desktop app: a client of the appliance. It signs in as the person,
//! holds only their session token (in the OS keyring), and makes every
//! request itself, so the page never sees the token. It holds no provider
//! or tool credentials; approvals are made here, outside the agent's context.

mod display;
mod frames;
mod local;
mod look;
mod pin;
mod speech;
mod store;
mod update;

use base64::Engine;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

type Res<T> = Result<T, String>;

static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// This app's version, as its bundle declares it.
pub fn app_version() -> String {
    VERSION.get().cloned().unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

struct App {
    store: store::Store,
    /// Address, how the client was built (`pin:…` or `device:…`), and the client.
    client: Mutex<Option<(String, String, reqwest::Client)>>,
    live: Mutex<HashMap<String, tokio::task::AbortHandle>>,
    speech: speech::Speech,
    /// The loopback address an app sign-in comes back to, open for one sign-in.
    loopback: Mutex<Option<tokio::net::TcpListener>>,
    /// The provider sign-in waiting on the browser: dropping it cancels the wait.
    browser_sign_in: Mutex<BrowserSignIn>,
}

impl App {
    /// How the display's socket and the local agent reach the appliance.
    fn trust(&self) -> Res<(String, Option<String>, Option<(String, String)>)> {
        let cfg = self.store.config();
        let url = cfg.appliance.ok_or("not connected to a server")?;
        if pin::is_public_appliance(&url) {
            let device = self.store.device(&url).ok_or("this computer is not paired with the server yet")?;
            return Ok((url, None, Some(device)));
        }
        let pinned = cfg.pins.get(&url).cloned();
        Ok((url, pinned, None))
    }

    /// `None` for plain http, which the appliance accepts only from itself.
    fn display_tls(&self) -> Res<(String, Option<rustls::ClientConfig>)> {
        let (url, pinned, device) = self.trust()?;
        if !url.starts_with("https://") {
            return Ok((url, None));
        }
        let mut tls = match device {
            Some((cert, key)) => pin::public_tls(&cert, &key)?,
            None => pin::tls(Some(pinned.as_deref().ok_or("the server's certificate is not trusted yet")?))?,
        };
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok((url, Some(tls)))
    }

    /// The address, the sign-in, the pin and the device certificate.
    fn local_session(&self) -> Res<(String, String, Option<String>, Option<(String, String)>)> {
        let (url, pinned, device) = self.trust()?;
        let token = self.store.token(&url).ok_or("signed out")?;
        Ok((url, token, pinned, device))
    }

    /// The appliance's address and a client for it: the pinned certificate
    /// on a private address, or the public roots and this computer's device
    /// certificate on magnus.pyrallon.ai.
    fn conn(&self) -> Res<(String, reqwest::Client)> {
        use sha2::Digest;
        let cfg = self.store.config();
        let url = cfg.appliance.ok_or("not connected to a server")?;
        let public = pin::is_public_appliance(&url);
        let device = if public { Some(self.store.device(&url).ok_or("this computer is not paired with the server yet")?) } else { None };
        let pinned = if public { None } else { cfg.pins.get(&url).cloned() };
        if !public && url.starts_with("https://") && pinned.is_none() {
            return Err("the server's certificate is not trusted yet".into());
        }
        let mode = match &device {
            Some((cert, _)) => format!("device:{}", hex::encode(sha2::Sha256::digest(cert.as_bytes()))),
            None => format!("pin:{}", pinned.as_deref().unwrap_or("")),
        };
        let mut c = self.client.lock().unwrap();
        if let Some((u, m, client)) = c.as_ref() {
            if *u == url && *m == mode {
                return Ok((url.clone(), client.clone()));
            }
        }
        let client = if let Some((cert, key)) = &device { pin::public_client(cert, key)? } else { pin::client(pinned.as_deref())? };
        *c = Some((url.clone(), mode, client.clone()));
        Ok((url, client))
    }

    fn session(&self) -> Res<(String, reqwest::Client, String)> {
        let (url, c) = self.conn()?;
        let token = self.store.token(&url).ok_or("signed out")?;
        Ok((url, c, token))
    }
}

/// An appliance address: scheme and host (and port), nothing else.
fn normalize(raw: &str) -> Res<String> {
    let raw = raw.trim();
    let with_scheme = if raw.contains("://") { raw.to_string() } else { format!("https://{raw}") };
    let u = url::Url::parse(&with_scheme).map_err(|_| format!("{raw} is not an address"))?;
    if !matches!(u.scheme(), "https" | "http") || u.host_str().is_none() {
        return Err(format!("{raw} is not an http or https address"));
    }
    Ok(u.origin().ascii_serialization())
}

async fn body_of(r: reqwest::Response) -> (u16, Value) {
    let status = r.status().as_u16();
    let text = r.text().await.unwrap_or_default();
    let body = if text.is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap_or(Value::String(text)) };
    (status, body)
}

fn message(body: &Value, status: u16) -> String {
    body["error"].as_str().or(body["message"].as_str()).or(body.as_str()).map(String::from).unwrap_or_else(|| format!("the appliance answered {status}"))
}

#[tauri::command]
async fn restore(app: State<'_, App>) -> Res<Value> {
    let Ok((url, c, token)) = app.session() else { return Ok(Value::Null) };
    match c.get(format!("{url}/v1/me")).bearer_auth(&token).send().await {
        Ok(r) if r.status().is_success() => Ok(json!({ "person": body_of(r).await.1, "appliance": url })),
        Ok(r) if r.status().as_u16() == 401 => {
            let (s, b) = body_of(r).await;
            let msg = message(&b, s);
            if msg.contains("not paired") {
                app.store.clear_device(&url);
                return Ok(json!({ "note": msg }));
            }
            app.store.clear_token(&url);
            Ok(json!({ "note": "Your session ended. Sign in again." }))
        }
        Ok(r) if r.status().as_u16() == 403 && pin::is_public_appliance(&url) => {
            let _ = body_of(r).await;
            app.store.clear_device(&url);
            app.store.clear_token(&url);
            Ok(json!({ "note": "This computer is no longer paired with the server. Pair it again from the private address." }))
        }
        // The appliance no longer accepts this version; the updater still can reach it.
        Ok(r) if r.status().as_u16() == 426 => Ok(json!({ "update_required": body_of(r).await.1, "appliance": url })),
        Ok(r) => {
            let (s, b) = body_of(r).await;
            Ok(json!({ "note": message(&b, s) }))
        }
        Err(e) => Ok(json!({ "note": format!("Cannot reach {url}: {}", pin::describe(&e)) })),
    }
}

#[tauri::command]
async fn connect(app: State<'_, App>, url: String) -> Res<Value> {
    let url = normalize(&url)?;
    if pin::is_public_appliance(&url) {
        let Some((cert, key)) = app.store.device(&url) else { return Ok(json!({ "status": "needs_device", "url": url })) };
        let c = pin::public_client(&cert, &key)?;
        let r = c.get(format!("{url}/v1/health")).send().await.map_err(|e| format!("Cannot reach {url}: {}", pin::describe(&e)))?;
        let (status, body) = body_of(r).await;
        // 401 is the origin refusing a fingerprint; 403 is Cloudflare blocking
        // a missing or revoked certificate before the origin sees it.
        if (status == 401 && message(&body, status).contains("not paired")) || status == 403 {
            app.store.clear_device(&url);
            return Ok(json!({ "status": "needs_device", "url": url }));
        }
        if body.get("ok").is_some() && body.get("version").is_some() {
            app.store.set_appliance(&url)?;
            return Ok(json!({ "status": "trusted", "url": url }));
        }
        return Err(format!("Cannot reach {url}: {}", message(&body, status)));
    }
    if url.starts_with("http://") {
        let c = pin::client(None)?;
        c.get(format!("{url}/v1/health")).send().await.map_err(|e| format!("Cannot reach {url}: {}", pin::describe(&e)))?;
        app.store.set_appliance(&url)?;
        return Ok(json!({ "status": "trusted", "url": url }));
    }
    let fp = pin::look(&url).await.map_err(|e| format!("Cannot reach {url}: {e}"))?;
    let status = match app.store.config().pins.get(&url) {
        Some(p) if *p == fp => "trusted",
        Some(_) => "changed",
        None => "new",
    };
    if status == "trusted" {
        app.store.set_appliance(&url)?;
    }
    Ok(json!({ "status": status, "fingerprint": fp, "url": url }))
}

#[tauri::command]
async fn trust(app: State<'_, App>, url: String, fingerprint: String) -> Res<()> {
    let url = normalize(&url)?;
    // Trust only what the appliance presents now.
    let now = pin::look(&url).await?;
    if now != fingerprint {
        return Err("The certificate changed while you were checking it. Start again.".into());
    }
    app.store.set_pin(&url, &fingerprint)
}

/// Pair this computer with the appliance's public name. The key is made
/// here and stored here; the appliance returns only the certificate.
#[tauri::command]
async fn pair_device(app: State<'_, App>, public_url: String) -> Res<Value> {
    let public_url = normalize(&public_url)?;
    if !pin::is_public_appliance(&public_url) {
        return Err("pair the public address of the server".into());
    }
    if app.store.device(&public_url).is_some() {
        return Ok(json!({ "already": true, "public_url": public_url }));
    }
    let (url, c, token) = app.session()?;
    let (key, csr) = pin::begin_device()?;
    let r = c
        .post(format!("{url}/v1/devices"))
        .bearer_auth(&token)
        .json(&json!({ "name": machine_name(), "csr": csr }))
        .send()
        .await
        .map_err(|e| format!("Cannot reach {url}: {}", pin::describe(&e)))?;
    let (status, body) = body_of(r).await;
    if status >= 400 {
        return Err(message(&body, status));
    }
    let certificate = body["certificate"].as_str().ok_or("the server sent no certificate")?;
    app.store.set_device(&public_url, certificate, &key)?;
    Ok(json!({ "device": body["device"], "fingerprint": body["fingerprint"], "public_url": public_url }))
}

fn machine_name() -> String {
    #[cfg(unix)]
    if let Ok(s) = std::fs::read_to_string("/etc/hostname") {
        let s = s.trim();
        if !s.is_empty() && s.chars().count() <= 80 {
            return s.to_string();
        }
    }
    std::env::var("COMPUTERNAME").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "computer".into())
}

#[tauri::command]
async fn sign_in(app: State<'_, App>, person: String, password: String, code: Option<String>, machine_code: Option<String>) -> Res<Value> {
    let (url, c) = app.conn()?;
    let r = c
        .post(format!("{url}/v1/sign-in"))
        .json(&json!({ "person": person, "password": password, "code": code, "machine_code": machine_code, "client": "app" }))
        .send()
        .await
        .map_err(|e| format!("Cannot reach {url}: {}", pin::describe(&e)))?;
    let (status, body) = body_of(r).await;
    // Not refusals: the page asks for the code, or opens the browser.
    if body["code_needed"] == true || body["browser"] == true || body["machine_code_needed"] == true {
        return Ok(body);
    }
    if status >= 400 {
        return Err(message(&body, status));
    }
    let token = body["token"].as_str().ok_or("the server sent no session")?;
    app.store.set_token(&url, token)?;
    Ok(body["person"].clone())
}

/// How this appliance signs people in: "oidc" (with the provider's name) or "password".
#[tauri::command]
async fn sign_in_method(app: State<'_, App>) -> Res<Value> {
    let (url, c) = app.conn()?;
    let r = c.get(format!("{url}/v1/sign-in")).send().await.map_err(|e| format!("Cannot reach {url}: {}", pin::describe(&e)))?;
    let (status, body) = body_of(r).await;
    if status >= 400 {
        return Err(message(&body, status));
    }
    Ok(body)
}

#[tauri::command]
async fn sign_out(app: State<'_, App>) -> Res<()> {
    if let Ok((url, c, token)) = app.session() {
        let _ = c.post(format!("{url}/v1/sign-out")).bearer_auth(&token).send().await;
        app.store.clear_token(&url);
    }
    Ok(())
}

/// Only the appliance's API, and never the calls that hand out a session.
fn allowed(path: &str) -> Res<()> {
    let refused = || Err(format!("{path} is not an appliance call this app makes"));
    // The request URL is normalised before it is sent (dot segments
    // removed), so the path is judged as it will be sent: it must already be
    // in that form, with no encoded dot, slash or backslash to decode later.
    let route = path.split('?').next().unwrap_or_default();
    let lower = route.to_ascii_lowercase();
    if route.contains('\\') || ["%2e", "%2f", "%5c"].iter().any(|e| lower.contains(e)) {
        return refused();
    }
    let normal = url::Url::parse("https://appliance.invalid").ok().and_then(|b| b.join(route).ok()).map(|u| u.path().to_string());
    if normal.as_deref() != Some(route) {
        return refused();
    }
    if !route.starts_with("/v1/") || path.contains('#') || route.starts_with("/v1/sign-in") || route.starts_with("/v1/sign-out") || route.starts_with("/v1/devices") {
        return refused();
    }
    Ok(())
}

#[tauri::command]
async fn api(app: State<'_, App>, method: String, path: String, body: Option<Value>) -> Res<Value> {
    allowed(&path)?;
    let (url, c, token) = app.session()?;
    let m = match method.as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        other => return Err(format!("{other} is not a method this app uses")),
    };
    let mut req = c.request(m, format!("{url}{path}")).bearer_auth(&token);
    if let Some(b) = body.filter(|b| !b.is_null()) {
        req = req.json(&b);
    }
    let r = req.send().await.map_err(|e| format!("Cannot reach {url}: {}", pin::describe(&e)))?;
    let (status, body) = body_of(r).await;
    Ok(json!({ "status": status, "body": body }))
}

/// Bytes come only from a file: a task's delivered output, a library file,
/// or a chat message exported as one.
fn file_path(path: &str) -> Res<()> {
    allowed(path)?;
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let output = parts.len() == 5 && parts[1] == "tasks" && parts[3] == "outputs";
    let library = parts.len() == 3 && parts[1] == "files" && !parts[2].is_empty();
    let export = parts.len() == 6 && parts[1] == "chat" && parts[3] == "entries" && parts[5] == "export";
    if !(output || library || export) {
        return Err("not a file".into());
    }
    Ok(())
}

async fn file_response(app: &App, path: &str) -> Res<reqwest::Response> {
    file_path(path)?;
    let (url, c, token) = app.session()?;
    let r = c.get(format!("{url}{path}")).bearer_auth(&token).send().await.map_err(|e| pin::describe(&e))?;
    if !r.status().is_success() {
        let (s, b) = body_of(r).await;
        return Err(message(&b, s));
    }
    Ok(r)
}
async fn bytes(app: &App, path: &str) -> Res<Vec<u8>> {
    file_response(app, path).await?.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
}
#[derive(serde::Serialize)]
struct FileBytes { data_b64: String, media_type: String }
#[tauri::command]
async fn put_bytes(app: State<'_, App>, path: String, data_b64: String) -> Res<()> {
    use base64::Engine;
    allowed(&path)?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data_b64).map_err(|e| e.to_string())?;
    let (url, c, token) = app.session()?;
    let r = c.put(format!("{url}{path}")).bearer_auth(&token).body(bytes).send().await.map_err(|e| format!("Cannot reach {url}: {}", pin::describe(&e)))?;
    let (status, body) = body_of(r).await;
    if status >= 400 {
        return Err(message(&body, status));
    }
    Ok(())
}

#[tauri::command]
async fn fetch_bytes(app: State<'_, App>, path: String) -> Res<FileBytes> {
    let response = file_response(&app, &path).await?;
    let media_type = response.headers().get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_owned();
    let data = response.bytes().await.map_err(|e| e.to_string())?;
    Ok(FileBytes { data_b64: base64::engine::general_purpose::STANDARD.encode(data), media_type })
}
/// Save a delivered file in the person's Downloads folder.
#[tauri::command]
async fn download(app: State<'_, App>, path: String, name: String) -> Res<String> {
    let data = bytes(&app, &path).await?;
    let safe: String = std::path::Path::new(&name)
        .file_name()
        .map(|n| n.to_string_lossy().chars().map(|c| if c.is_alphanumeric() || " ._-()".contains(c) { c } else { '_' }).collect())
        .filter(|n: &String| !n.trim_matches('.').is_empty())
        .unwrap_or_else(|| "file".into());
    let dir = dirs::download_dir().or_else(dirs::home_dir).ok_or("no Downloads folder")?;
    let (stem, ext) = match safe.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (safe.clone(), String::new()),
    };
    let mut target = dir.join(&safe);
    let mut n = 1;
    while target.exists() {
        target = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    std::fs::write(&target, data).map_err(|e| format!("save {}: {e}", target.display()))?;
    Ok(target.display().to_string())
}

/// Relay a task's live text to the page as it streams.
#[tauri::command]
async fn live_start(handle: AppHandle, app: State<'_, App>, task: String) -> Res<()> {
    if task.is_empty() || !task.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err("not a task id".into());
    }
    let (url, c, token) = app.session()?;
    let t = task.clone();
    let job = tokio::spawn(async move {
        let Ok(r) = c.get(format!("{url}/v1/tasks/{t}/live")).bearer_auth(&token).send().await else { return };
        let mut stream = r.bytes_stream();
        let mut buf = String::new();
        while let Some(Ok(chunk)) = stream.next().await {
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(end) = buf.find("\n\n") {
                let event: String = buf.drain(..end + 2).collect();
                for line in event.lines() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if let Ok(v) = serde_json::from_str::<Value>(data) {
                            let _ = handle.emit("live", json!({ "task": t, "text": v["text"] }));
                        }
                    }
                }
            }
        }
    });
    if let Some(old) = app.live.lock().unwrap().insert(task, job.abort_handle()) {
        old.abort();
    }
    Ok(())
}

#[tauri::command]
fn live_stop(app: State<'_, App>, task: String) {
    if let Some(j) = app.live.lock().unwrap().remove(&task) {
        j.abort();
    }
}

#[tauri::command]
fn notify(handle: AppHandle, title: String, body: String) -> Res<()> {
    handle.notification().builder().title(title).body(body).show().map_err(|e| e.to_string())
}

#[tauri::command]
async fn speak(app: State<'_, App>, text: String) -> Res<()> {
    app.speech.speak(&text).await
}

#[tauri::command]
async fn stop_speaking(app: State<'_, App>) -> Res<()> {
    app.speech.stop().await;
    Ok(())
}

/// This app's version and the appliance's, as each declares it.
#[tauri::command]
async fn app_info(app: State<'_, App>) -> Res<Value> {
    let appliance = match app.conn() {
        Ok((url, c)) => match c.get(format!("{url}/v1/version")).send().await {
            Ok(r) => body_of(r).await.1,
            Err(_) => Value::Null,
        },
        Err(_) => Value::Null,
    };
    Ok(json!({ "app": app_version(), "appliance": appliance }))
}

/// Open a provider's sign-in page in the person's browser.
#[tauri::command]
fn open_url(handle: AppHandle, url: String) -> Res<()> {
    let u = url::Url::parse(&url).map_err(|_| "not a link")?;
    if u.scheme() != "https" && u.scheme() != "http" {
        return Err("only web links open from here".into());
    }
    handle.opener().open_url(u.as_str(), None::<&str>).map_err(|e| e.to_string())
}

/// Where a sign-in listens. None: any free port, and the address told to
/// the provider is `http://127.0.0.1:<that port>/callback`. Some: the
/// provider registered that address exactly (Slack does), so this is the
/// only one that works.
fn sign_in_listen(redirect: Option<&str>) -> Res<String> {
    let Some(redirect) = redirect.filter(|s| !s.is_empty()) else { return Ok("127.0.0.1:0".into()) };
    let u = url::Url::parse(redirect).map_err(|_| "the sign-in address is not a link".to_string())?;
    let port = u.port().ok_or("the sign-in address names no port")?;
    if u.scheme() != "http" || u.host_str() != Some("127.0.0.1") || u.path() != "/callback" {
        return Err("the sign-in address must be http://127.0.0.1:<port>/callback on this computer".into());
    }
    Ok(format!("127.0.0.1:{port}"))
}

/// Open a loopback address on this computer for one sign-in to one of the
/// person's apps (RFC 8252): the app's sign-in page sends its code here.
/// `redirect`, when the connector names one, is listened on exactly.
#[tauri::command]
async fn loopback_open(app: State<'_, App>, redirect: Option<String>) -> Res<String> {
    let bind = sign_in_listen(redirect.as_deref())?;
    let l = tokio::net::TcpListener::bind(&bind).await.map_err(|e| format!("cannot listen on this computer ({bind}): {e}"))?;
    let back = match redirect.filter(|s| !s.is_empty()) {
        Some(u) => u,
        None => format!("http://127.0.0.1:{}/callback", l.local_addr().map_err(|e| e.to_string())?.port()),
    };
    *app.loopback.lock().unwrap() = Some(l);
    Ok(back)
}

/// How long a sign-in page may take before the app stops waiting.
const SIGN_IN_WAIT: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Wait for the sign-in page's redirect: its code, its state and the
/// server's name when it gives one (RFC 9207, as Google does: without it the
/// appliance refuses the answer as a possible mix-up), or its error.
#[tauri::command]
async fn loopback_wait(app: State<'_, App>) -> Res<Value> {
    let l = app.loopback.lock().unwrap().take().ok_or("no sign-in is waiting")?;
    let q = redirected(l, SIGN_IN_WAIT).await?;
    Ok(json!({ "code": q["code"], "state": q["state"], "iss": q.get("iss") }))
}

/// The query of the first redirect to `/callback` on this listener: it has
/// a code and state, or the sign-in page's error. The browser is told to
/// return to Magnus either way.
async fn redirected(l: tokio::net::TcpListener, wait: std::time::Duration) -> Res<HashMap<String, String>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let wait_for = async {
        loop {
            let (mut s, _) = l.accept().await.map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; 8192];
            let mut n = 0;
            while n < buf.len() {
                let k = s.read(&mut buf[n..]).await.map_err(|e| e.to_string())?;
                if k == 0 {
                    break;
                }
                n += k;
                if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let head = String::from_utf8_lossy(&buf[..n]);
            let target = head.lines().next().and_then(|l| l.strip_prefix("GET ")).and_then(|l| l.split(' ').next()).unwrap_or_default().to_string();
            let Ok(u) = url::Url::parse(&format!("http://127.0.0.1{target}")) else { continue };
            if u.path() != "/callback" {
                let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await;
                continue;
            }
            let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
            let (title, result) = match (q.get("code"), q.get("state"), q.get("error")) {
                (Some(_), Some(_), _) => ("Signed in. You can close this page and return to Magnus.", Ok(q.clone())),
                (_, _, Some(e)) => ("The sign-in did not finish. Return to Magnus.", Err(format!("the sign-in page said: {}", q.get("error_description").unwrap_or(e)))),
                _ => ("The sign-in did not finish. Return to Magnus.", Err("the sign-in page sent no code".to_string())),
            };
            let body = format!("<!doctype html><meta charset=utf-8><title>Magnus</title><p style=\"font:16px system-ui;margin:40px\">{title}</p>");
            let _ = s.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
            return result;
        }
    };
    tokio::time::timeout(wait, wait_for).await.map_err(|_| "the sign-in took too long; start again".to_string())?
}

/// Sign in through the appliance's identity provider in the person's own
/// browser (RFC 8252): the browser comes back to a loopback address here
/// with a code, which only the appliance can exchange (it holds the PKCE
/// verifier). Everything goes over the pinned connection to the appliance;
/// the session goes to the keyring and the page gets only who signed in.
#[tauri::command]
async fn provider_sign_in(handle: AppHandle, app: State<'_, App>, login_hint: Option<String>) -> Res<Value> {
    let (url, c) = app.conn()?;
    // One at a time, from the first request: a new sign-in, or the person's
    // Cancel (shown as soon as this starts), ends this one.
    let (n, mut cancel) = app.browser_sign_in.lock().unwrap().begin();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|e| format!("cannot listen on this computer: {e}"))?;
    let port = l.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let begin = async { c.post(format!("{url}/v1/sign-in/browser")).json(&json!({ "redirect_uri": redirect_uri, "login_hint": login_hint, "client": "app" })).send().await.map_err(|e| format!("Cannot reach {url}: {}", pin::describe(&e))) };
    let r = unless_cancelled(begin, &mut cancel).await?;
    let (status, begun) = body_of(r).await;
    if status >= 400 {
        return Err(message(&begun, status));
    }
    let (Some(state), Some(page)) = (begun["state"].as_str(), begun["authorize_url"].as_str()) else { return Err("the server sent no sign-in page".into()) };
    let page = url::Url::parse(page).map_err(|_| "the server sent a sign-in page that is not a link")?;
    if page.scheme() != "https" && page.scheme() != "http" {
        return Err("the server sent a sign-in page that is not a web link".into());
    }
    if !matches!(cancel.try_recv(), Err(tokio::sync::oneshot::error::TryRecvError::Empty)) {
        return Err(CANCELLED.into());
    }
    handle.opener().open_url(page.as_str(), None::<&str>).map_err(|e| format!("cannot open the browser: {e}"))?;
    // As long as the appliance keeps the sign-in open.
    let wait = std::time::Duration::from_secs(begun["expires_in"].as_u64().unwrap_or(SIGN_IN_WAIT.as_secs()));
    let q = unless_cancelled(redirected(l, wait), &mut cancel).await?;
    if q.get("state").map(String::as_str) != Some(state) {
        return Err("the browser came back from a different sign-in; start again".into());
    }
    let finish = json!({ "state": state, "code": q["code"], "iss": q.get("iss") });
    let r = c.post(format!("{url}/v1/sign-in/browser/finish")).json(&finish).send().await.map_err(|e| format!("Cannot reach {url}: {}", pin::describe(&e)))?;
    let (status, body) = body_of(r).await;
    if status >= 400 {
        return Err(message(&body, status));
    }
    let token = body["token"].as_str().ok_or("the server sent no session")?;
    // Kept only if this is still the sign-in under way, checked and stored
    // under one lock, so neither a Cancel nor a newer sign-in can come in
    // between. Otherwise its session is ended there and never stored, so it
    // cannot replace the one the person chose.
    let kept = {
        let mut current = app.browser_sign_in.lock().unwrap();
        if current.finish(n) {
            app.store.set_token(&url, token)?;
            true
        } else {
            false
        }
    };
    if !kept {
        let _ = c.post(format!("{url}/v1/sign-out")).bearer_auth(token).send().await;
        return Err(CANCELLED.into());
    }
    Ok(json!({ "person": body["person"] }))
}

const CANCELLED: &str = "the sign-in was cancelled";

/// The browser sign-in under way, if any: each has its number, and only the
/// latest, not cancelled, may finish.
#[derive(Default)]
struct BrowserSignIn {
    latest: u64,
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
}

impl BrowserSignIn {
    /// Start one: the one before is cancelled (its sender dropped).
    fn begin(&mut self) -> (u64, tokio::sync::oneshot::Receiver<()>) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.latest += 1;
        self.cancel = Some(tx);
        (self.latest, rx)
    }

    /// The person's Cancel: the one under way ends, and none may finish.
    fn cancel(&mut self) {
        self.cancel = None;
    }

    /// Whether sign-in `n` may finish: it is the latest and not cancelled.
    /// It is then done, so a Cancel after it changes nothing.
    fn finish(&mut self, n: u64) -> bool {
        let ok = n == self.latest && self.cancel.is_some();
        if ok {
            self.cancel = None;
        }
        ok
    }
}

/// `wait`, unless the sign-in is cancelled (its sender sent or dropped) first.
async fn unless_cancelled<T>(wait: impl std::future::Future<Output = Res<T>>, cancel: &mut tokio::sync::oneshot::Receiver<()>) -> Res<T> {
    tokio::select! {
        r = wait => r,
        _ = cancel => Err(CANCELLED.into()),
    }
}

/// The person cancelled the provider sign-in: nothing it brings back is kept.
#[tauri::command]
async fn provider_sign_in_cancel(app: State<'_, App>) -> Res<()> {
    app.browser_sign_in.lock().unwrap().cancel();
    Ok(())
}

pub fn run() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(update::Updates::default())
        .setup(|app| {
            let _ = VERSION.set(app.package_info().version.to_string());
            update::start(app.handle().clone());
            Ok(())
        })
        .manage(App { store: store::Store::open(), client: Mutex::new(None), live: Mutex::new(HashMap::new()), speech: speech::Speech::default(), loopback: Mutex::new(None), browser_sign_in: Mutex::new(BrowserSignIn::default()) })
        .manage(display::Displays::default())
        .manage(frames::Frames::default())
        .manage(local::Local::default())
        .register_uri_scheme_protocol(frames::SCHEME, |ctx, request| {
            use tauri::Manager;
            frames::serve(&ctx.app_handle().state::<frames::Frames>(), request.uri().path())
        })
        .invoke_handler(tauri::generate_handler![put_bytes, display::display_open, display::display_send, display::display_close, frames::sandbox_frame, local::local_state, local::local_start, local::local_pause, local::local_stop, restore, connect, trust, sign_in_method, sign_in, pair_device, provider_sign_in, provider_sign_in_cancel, sign_out, api, fetch_bytes, download, live_start, live_stop, notify, speak, stop_speaking, open_url, loopback_open, loopback_wait, app_info, update::update_state, update::update_look, update::update_install, update::whats_new, look::set_material, look::system_look])
        .run(tauri::generate_context!())
        .expect("start the Magnus app");
}

#[cfg(test)]
mod tests {
    /// A cancelled provider sign-in stops waiting at once, so the browser
    /// finishing it later brings back nothing that is kept.
    #[tokio::test]
    async fn a_cancelled_browser_sign_in_stops_waiting() {
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
        let forever = async { tokio::time::sleep(std::time::Duration::from_secs(3600)).await; Ok::<_, String>(()) };
        drop(tx);
        let r = tokio::time::timeout(std::time::Duration::from_secs(5), super::unless_cancelled(forever, &mut rx)).await;
        assert_eq!(r, Ok(Err(super::CANCELLED.to_string())), "the sign-in kept waiting after it was cancelled");
    }

    /// Only the latest sign-in, not cancelled, finishes: a Cancel at any
    /// point, even before the appliance answered the first request, stops it,
    /// and an older one never takes a newer one's place (review of #160).
    #[test]
    fn only_the_latest_uncancelled_sign_in_finishes() {
        let mut s = super::BrowserSignIn::default();
        let (first, mut rx1) = s.begin();
        s.cancel();
        assert!(rx1.try_recv().is_err_and(|e| e == tokio::sync::oneshot::error::TryRecvError::Closed), "a Cancel before the first answer did not reach the sign-in");
        assert!(!s.finish(first), "a cancelled sign-in finished");

        let (older, mut rx2) = s.begin();
        let (newer, mut rx3) = s.begin();
        assert!(rx2.try_recv().is_err_and(|e| e == tokio::sync::oneshot::error::TryRecvError::Closed), "a newer sign-in did not end the older one");
        assert!(!s.finish(older), "an older sign-in finished in a newer one's place");
        assert_eq!(rx3.try_recv(), Err(tokio::sync::oneshot::error::TryRecvError::Empty), "the older one's finish ended the newer one");
        assert!(s.finish(newer));
        s.cancel();
        assert!(!s.finish(newer), "a sign-in finished twice");
    }

    use super::*;

    #[test]
    fn the_page_cannot_reach_beyond_the_api_or_take_a_session() {
        for bad in ["/v1/sign-in", "/v1/sign-in/device", "/v1/sign-in/device/signin_1", "/v1/sign-out", "/v1/devices", "/v1/devices/dev_1", "/v1/../etc", "https://evil.example/v1/x", "/other", "/v1/x#y"] {
            assert!(allowed(bad).is_err(), "{bad} was allowed");
        }
        // The request URL is normalised before it is sent: a dot segment or
        // an encoded dot or slash must not hide a sign-in call.
        for bad in ["/v1/./sign-in/device", "/v1/%2e/sign-in", "/v1/%2E/sign-out", "/v1/x/%2e%2e/devices", "/v1/x/.%2E/sign-in", "/v1/%2fsign-in", "/v1\\sign-in", "/v1/x?y#z"] {
            assert!(allowed(bad).is_err(), "{bad} was allowed");
        }
        assert!(allowed("/v1/inbox").is_ok());
        assert!(allowed("/v1/tasks?status=open").is_ok());
        assert!(file_path("/v1/tasks/t1/outputs/o1").is_ok());
        assert!(file_path("/v1/files/file_1").is_ok());
        assert!(file_path("/v1/chat/th1/entries/a:t1/export").is_ok());
        assert!(file_path("/v1/chat/th1/entries/a:t1/share").is_err());
        assert!(file_path("/v1/tasks/t1").is_err());
        assert!(file_path("/v1/files").is_err());
        assert!(file_path("/v1/files/file_1/info").is_err());
    }

    #[test]
    fn a_registered_sign_in_address_is_listened_on_exactly() {
        assert_eq!(sign_in_listen(None).unwrap(), "127.0.0.1:0");
        assert_eq!(sign_in_listen(Some("")).unwrap(), "127.0.0.1:0");
        assert_eq!(sign_in_listen(Some("http://127.0.0.1:17321/callback")).unwrap(), "127.0.0.1:17321");
        assert!(sign_in_listen(Some("http://localhost:17321/callback")).is_err());
        assert!(sign_in_listen(Some("https://magnus.pyrallon.ai/oauth/slack/callback")).is_err());
    }

    #[test]
    fn an_address_is_reduced_to_its_origin() {
        assert_eq!(normalize("magnus.local:7443").unwrap(), "https://magnus.local:7443");
        assert_eq!(normalize("https://10.0.0.5:7443/some/path").unwrap(), "https://10.0.0.5:7443");
        assert_eq!(normalize("http://127.0.0.1:7700/").unwrap(), "http://127.0.0.1:7700");
        assert!(normalize("file:///etc/passwd").is_err());
    }

    #[test]
    fn the_public_name_is_not_pinned_and_the_device_key_stays_here() {
        assert!(pin::is_public_appliance("https://magnus.pyrallon.ai"));
        assert!(pin::is_public_appliance("https://Magnus.Pyrallon.ai/v1/me"));
        assert!(!pin::is_public_appliance("https://10.1.1.5:7700"));
        assert!(!pin::is_public_appliance("https://magnus.local:7700"));
        assert!(!pin::is_public_appliance("https://strix.tailnet.ts.net"));
        assert!(!pin::is_public_appliance("http://magnus.pyrallon.ai"));
        let (key, csr) = pin::begin_device().unwrap();
        assert!(key.contains("PRIVATE KEY"));
        assert!(csr.contains("BEGIN CERTIFICATE REQUEST"));
        assert!(!csr.contains("PRIVATE KEY"), "the request sent to the server carried the key");
    }
}
