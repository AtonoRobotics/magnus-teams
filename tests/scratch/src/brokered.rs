//! For brokered connectors' tests: a server image built from this checkout
//! (`brokered/build.sh`), the config that runs it, and a local host
//! standing in for the provider's API (the provider's real API needs a
//! person's account at it; the contract tests run against one).

use magnus_invariants::appliance::workspace_root;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// A server image, built: where it is and its digest (`sha256:<hex>`).
pub struct Image {
    pub dir: tempfile::TempDir,
    pub digest: String,
}

impl Image {
    /// The image's file, named by its digest.
    pub fn file(&self) -> std::path::PathBuf {
        self.dir.path().join(format!("{}.cpio", self.digest.trim_start_matches("sha256:")))
    }
}

/// Build `package`'s server image as a release does.
pub fn image(package: &str) -> Image {
    build(package, None)
}

/// Build it with its own target directory, as another machine would.
pub fn image_elsewhere(package: &str, target: &Path) -> Image {
    build(package, Some(target))
}

fn build(package: &str, target: Option<&Path>) -> Image {
    let dir = tempfile::tempdir().unwrap();
    let mut c = std::process::Command::new(workspace_root().join("brokered/build.sh"));
    c.arg(package).arg(dir.path());
    if let Some(t) = target {
        c.env("CARGO_TARGET_DIR", t);
    }
    let out = c.output().expect("run brokered/build.sh");
    assert!(out.status.success(), "building {package}'s image failed:\n{}", String::from_utf8_lossy(&out.stderr));
    let digest = String::from_utf8(out.stdout).unwrap().trim().to_string();
    assert!(digest.starts_with("sha256:"), "{digest}");
    Image { dir, digest }
}

/// The config that runs `image` as the connector `name`, reaching `hosts`,
/// whose people sign in at `issuer`, with its tools' classes; instances
/// stop after `idle_s` unused.
pub fn configure(cfg: &mut toml::Table, image: &Image, idle_s: u64, name: &str, hosts: &[String], issuer: &str, tools: &[(&str, &str)]) {
    let mut b = toml::Table::new();
    b.insert("images".into(), image.dir.path().to_str().unwrap().into());
    b.insert("idle_s".into(), (idle_s as i64).into());
    cfg.insert("brokered".into(), b.into());
    let mut c = toml::Table::new();
    for (k, v) in [
        ("name", name),
        ("transport", "brokered"),
        ("url", &image.digest),
        ("issuer", issuer),
        ("auth", "oauth"),
        ("audience", "private"),
        ("label", "private+untrusted"),
        ("idempotency", "none"),
    ] {
        c.insert(k.into(), v.into());
    }
    c.insert("credential".into(), format!("connector_{name}").into());
    c.insert("hosts".into(), toml::Value::Array(hosts.iter().map(|h| h.as_str().into()).collect()));
    let mut t = toml::Table::new();
    for (tool, risk) in tools {
        let mut r = toml::Table::new();
        r.insert("risk".into(), (*risk).into());
        t.insert((*tool).into(), r.into());
    }
    c.insert("tools".into(), t.into());
    let list = cfg.entry("connectors").or_insert_with(|| toml::Value::Array(vec![]));
    list.as_array_mut().unwrap().push(c.into());
}

/// How many brokered microVMs are running on the appliance: each has its
/// folder under the state directory while it runs.
pub fn running(state_dir: &std::path::Path) -> usize {
    std::fs::read_dir(state_dir.join("brk")).map(|d| d.count()).unwrap_or(0)
}

/// A CA of the test's own: its certificate, DER, in a file
/// (`[brokered] roots`), and an acceptor with a certificate from it for
/// `name`.
pub struct TestCa {
    pub root: std::path::PathBuf,
    cert: rcgen::Certificate,
    key: rcgen::KeyPair,
    _dir: tempfile::TempDir,
}

impl TestCa {
    pub fn new(cn: &str) -> TestCa {
        let dir = tempfile::tempdir().unwrap();
        let mut p = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        p.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        p.distinguished_name.push(rcgen::DnType::CommonName, cn);
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = p.self_signed(&key).unwrap();
        let root = dir.path().join("root.der");
        std::fs::write(&root, cert.der()).unwrap();
        TestCa { root, cert, key, _dir: dir }
    }

    /// TLS for a host named `name` (a DNS name or an IP address).
    pub fn acceptor(&self, name: &str) -> tokio_rustls::TlsAcceptor {
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = rcgen::CertificateParams::new(vec![name.to_string()]).unwrap().signed_by(&key, &self.cert, &self.key).unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert.der().clone()], rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into()))
            .unwrap();
        tokio_rustls::TlsAcceptor::from(Arc::new(tls))
    }
}

/// `cfg`'s servers trust `ca`'s certificates too.
pub fn trust(cfg: &mut toml::Table, ca: &TestCa) {
    let b = cfg.entry("brokered").or_insert_with(|| toml::Table::new().into()).as_table_mut().unwrap();
    let roots = b.entry("roots").or_insert_with(|| toml::Value::Array(vec![])).as_array_mut().unwrap();
    roots.push(ca.root.display().to_string().into());
}

/// A local host standing in for a provider's API, over HTTPS with a
/// certificate for `127.0.0.1` from its own CA: every request it is sent,
/// as (path, Authorization header), answered with a greeting (with JSON
/// under `/json/`); and every
/// byte that reached it before TLS, as it came.
#[derive(Clone)]
pub struct ProviderApi {
    /// `127.0.0.1:<port>`, as a manifest's hosts name it.
    pub host: String,
    pub seen: Arc<Mutex<Vec<(String, String)>>>,
    pub raw: Arc<Mutex<Vec<u8>>>,
    pub ca: Arc<TestCa>,
    /// What the provider lists at `/items`, a JSON array (the test
    /// server's `list` reads it).
    pub items: Arc<Mutex<String>>,
}

impl ProviderApi {
    pub async fn start() -> ProviderApi {
        Self::serve(true).await
    }

    /// One that answers HTTP in the clear, as a wrong host might.
    pub async fn in_the_clear() -> ProviderApi {
        Self::serve(false).await
    }

    async fn serve(tls: bool) -> ProviderApi {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        async fn answer(
            mut s: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
            seen: &Mutex<Vec<(String, String)>>,
            raw: Option<&Mutex<Vec<u8>>>,
            items: &Mutex<String>,
        ) {
            let mut got = vec![];
            let mut b = [0u8; 4096];
            while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = match s.read(&mut b).await {
                    Ok(n) if n > 0 => n,
                    _ => return,
                };
                got.extend_from_slice(&b[..n]);
                if let Some(r) = raw {
                    r.lock().unwrap().extend_from_slice(&b[..n]);
                }
                // What is not HTTP is answered as an HTTP server does.
                if !got[0].is_ascii_uppercase() {
                    let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                    return;
                }
            }
            let head = String::from_utf8_lossy(&got).to_string();
            let path = head.split_whitespace().nth(1).unwrap_or_default().to_string();
            let auth =
                head.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("authorization")).map(|(_, v)| v.trim().to_string())).unwrap_or_default();
            // A path under `/json/` is answered with JSON that is valid but
            // names nothing a tool's answer must (no `id`).
            // `/json/named-by-token` answers with the caller's own token as
            // a property name beside the answer it should give, as a provider
            // echoing it in an odd place would.
            let body = match path.as_str() {
                "/json/named-by-token" => format!(r#"{{"id":"sent","{}":true}}"#, auth.trim_start_matches("Bearer ")),
                p if p.starts_with("/json/") => r#"{"done":true}"#.to_string(),
                "/items" => items.lock().map(|i| i.clone()).unwrap_or_default(),
                _ => "hello from the provider".to_string(),
            };
            seen.lock().unwrap().push((path, auth));
            let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
            let _ = s.shutdown().await;
        }
        let ca = Arc::new(TestCa::new("A CA standing in for a provider's (test)"));
        let acceptor = ca.acceptor("127.0.0.1");
        let (seen, raw): (Arc<Mutex<Vec<(String, String)>>>, Arc<Mutex<Vec<u8>>>) = Default::default();
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let host = l.local_addr().unwrap().to_string();
        let items = Arc::new(Mutex::new("[]".to_string()));
        let (s2, r2, i2) = (seen.clone(), raw.clone(), items.clone());
        tokio::spawn(async move {
            while let Ok((tcp, _)) = l.accept().await {
                let (acceptor, seen, raw, items) = (acceptor.clone(), s2.clone(), r2.clone(), i2.clone());
                tokio::spawn(async move {
                    if !tls {
                        return answer(tcp, &seen, Some(&raw), &items).await;
                    }
                    let mut first = [0u8; 4096];
                    let n = tcp.peek(&mut first).await.unwrap_or(0);
                    raw.lock().unwrap().extend_from_slice(&first[..n]);
                    if let Ok(s) = acceptor.accept(tcp).await {
                        answer(s, &seen, None, &items).await;
                    }
                });
            }
        });
        ProviderApi { host, seen, raw, ca, items }
    }
}

/// A host no manifest names: how many connections ever reached it.
pub struct Elsewhere {
    pub host: String,
    pub reached: Arc<AtomicUsize>,
}

impl Elsewhere {
    pub async fn start() -> Elsewhere {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let host = l.local_addr().unwrap().to_string();
        let reached = Arc::new(AtomicUsize::new(0));
        let r = reached.clone();
        tokio::spawn(async move {
            while l.accept().await.is_ok() {
                r.fetch_add(1, Ordering::SeqCst);
            }
        });
        Elsewhere { host, reached }
    }
}

/// One request Gmail's stand-in was sent: the name the client asked for in
/// its TLS hello, the method, path, `Host`, `Authorization` and body.
#[derive(Clone, Debug)]
pub struct GmailRequest {
    pub sni: String,
    pub method: String,
    pub path: String,
    pub host: String,
    pub auth: String,
    pub body: String,
}

/// A local HTTPS host standing in for `gmail.googleapis.com` (the real one
/// needs a person's Google account; the contract test uses one), with a
/// certificate for that name from a CA of the test's own. It keeps drafts
/// and records each request; one answer may be set ahead.
pub struct StandInGmail {
    pub addr: String,
    /// The CA's certificate, DER, in a file: `[brokered] roots`.
    pub root: std::path::PathBuf,
    pub seen: Arc<Mutex<Vec<GmailRequest>>>,
    /// The next answer, instead of Gmail's usual one: status, headers, body.
    pub next: Arc<Mutex<Option<(u16, String, String)>>>,
    /// What every draft says when read: `to`, `cc`, `bcc`, `subject` and
    /// `body`, and an `html` part and an `attachment` (`filename`, `data`)
    /// where set. A test changes it as the person (or anyone with the
    /// mailbox) would edit the draft in Gmail.
    pub draft: Arc<Mutex<serde_json::Value>>,
    /// The next send's answer, instead of Gmail's usual one (the draft is
    /// read before it is sent, so `next` would answer that read).
    pub next_send: Arc<Mutex<Option<(u16, String, String)>>>,
    /// The next read of a draft, once `hold_draft_read` sets it.
    held: Arc<Mutex<Option<HeldRead>>>,
    _ca: TestCa,
}

/// A read of a draft Gmail's stand-in has and answers only once released.
#[derive(Clone)]
pub struct HeldRead {
    received: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

impl HeldRead {
    /// Resolves once the held read has reached Gmail.
    pub async fn received(&self) {
        self.received.notified().await
    }
    /// Let Gmail answer it.
    pub fn release(&self) {
        self.release.notify_one()
    }
}

impl StandInGmail {
    pub async fn start() -> StandInGmail {
        Self::with_drafts(0).await
    }

    /// One whose mailbox holds `drafts` drafts already (`d1`, ...), as if
    /// the person had written them.
    pub async fn with_drafts(drafts: usize) -> StandInGmail {
        let ca = TestCa::new("A CA standing in for Google's (test)");
        let (acceptor, root) = (ca.acceptor("gmail.googleapis.com"), ca.root.clone());
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let seen: Arc<Mutex<Vec<GmailRequest>>> = Arc::default();
        let next: Arc<Mutex<Option<(u16, String, String)>>> = Arc::default();
        let draft = Arc::new(Mutex::new(serde_json::json!({ "to": "Bo <bo@example.com>", "cc": "", "bcc": "", "subject": "Re: Plans", "body": "Friday works." })));
        let next_send: Arc<Mutex<Option<(u16, String, String)>>> = Arc::default();
        let held: Arc<Mutex<Option<HeldRead>>> = Arc::default();
        let (s2, n2, d2, ns2, h2) = (seen.clone(), next.clone(), draft.clone(), next_send.clone(), held.clone());
        tokio::spawn(async move {
            while let Ok((tcp, _)) = l.accept().await {
                let (acceptor, seen, next, draft, next_send, held) = (acceptor.clone(), s2.clone(), n2.clone(), d2.clone(), ns2.clone(), h2.clone());
                tokio::spawn(async move {
                    let Ok(mut s) = acceptor.accept(tcp).await else { return };
                    let sni = s.get_ref().1.server_name().unwrap_or_default().to_string();
                    let Some(r) = read_request(&mut s, sni).await else { return };
                    if r.method == "GET" && r.path.contains("/drafts/") {
                        let hold = held.lock().unwrap().take();
                        if let Some(h) = hold {
                            h.received.notify_one();
                            h.release.notified().await;
                        }
                    }
                    let send = r.method == "POST" && r.path.ends_with("/drafts/send");
                    let (status, head, body) = send
                        .then(|| next_send.lock().unwrap().take())
                        .flatten()
                        .or_else(|| next.lock().unwrap().take())
                        .unwrap_or_else(|| gmail_answer(&r, &seen.lock().unwrap(), drafts, &draft.lock().unwrap()));
                    seen.lock().unwrap().push(r);
                    let out = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{head}\r\n{body}", body.len());
                    use tokio::io::AsyncWriteExt;
                    let _ = s.write_all(out.as_bytes()).await;
                    let _ = s.shutdown().await;
                });
            }
        });
        StandInGmail { addr, root, seen, next, draft, next_send, held, _ca: ca }
    }

    /// The next read of a draft reaches Gmail and waits there, unanswered,
    /// until released.
    pub fn hold_draft_read(&self) -> HeldRead {
        let h = HeldRead { received: Arc::default(), release: Arc::default() };
        *self.held.lock().unwrap() = Some(h.clone());
        h
    }

    pub fn requests(&self) -> Vec<GmailRequest> {
        self.seen.lock().unwrap().clone()
    }
}

pub(crate) async fn read_request(s: &mut (impl tokio::io::AsyncRead + Unpin), sni: String) -> Option<GmailRequest> {
    use tokio::io::AsyncReadExt;
    let mut raw = vec![];
    let mut b = [0u8; 4096];
    let split = loop {
        let n = s.read(&mut b).await.ok().filter(|n| *n > 0)?;
        raw.extend_from_slice(&b[..n]);
        if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
    };
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let header = |name: &str| {
        head.lines().skip(1).find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim().to_string())).unwrap_or_default()
    };
    let len: usize = header("content-length").parse().unwrap_or(0);
    while raw.len() < split + 4 + len {
        let n = s.read(&mut b).await.ok().filter(|n| *n > 0)?;
        raw.extend_from_slice(&b[..n]);
    }
    let mut first = head.lines().next().unwrap_or_default().split_whitespace();
    let (method, path) = (first.next().unwrap_or_default().to_string(), first.next().unwrap_or_default().to_string());
    Some(GmailRequest { sni, method, path, host: header("host"), auth: header("authorization"), body: String::from_utf8_lossy(&raw[split + 4..split + 4 + len]).to_string() })
}

/// What Gmail answers, for the requests the tests make: one thread in the
/// inbox, drafts made `d1`, `d2`, ... (after those there already) and sent
/// only if made, each reading as `draft` says.
fn gmail_answer(r: &GmailRequest, before: &[GmailRequest], drafts: usize, draft: &serde_json::Value) -> (u16, String, String) {
    let base = "/gmail/v1/users/me";
    let path = r.path.strip_prefix(base).unwrap_or("");
    let made = drafts + before.iter().filter(|x| x.method == "POST" && x.path == format!("{base}/drafts")).count();
    let ok = |v: serde_json::Value| (200, String::new(), v.to_string());
    match (r.method.as_str(), path.split('?').next().unwrap_or("")) {
        ("GET", "/threads") => ok(serde_json::json!({ "threads": [{ "id": "t1", "snippet": "Can you send me the plan for Friday?" }] })),
        ("POST", "/drafts") => ok(serde_json::json!({ "id": format!("d{}", made + 1), "message": { "id": format!("dm{}", made + 1), "threadId": "t1", "labelIds": ["DRAFT"] } })),
        ("POST", p) if p.starts_with("/threads/") && p.ends_with("/modify") => {
            ok(serde_json::json!({ "id": "t1", "messages": [{ "id": "m1", "labelIds": ["INBOX", "Label_plans"] }] }))
        }
        ("GET", p) if p.strip_prefix("/drafts/d").and_then(|n| n.parse::<usize>().ok()).is_some_and(|n| (1..=made).contains(&n)) => {
            use base64::Engine;
            let n = &p["/drafts/d".len()..];
            let headers: Vec<serde_json::Value> = [("To", "to"), ("Cc", "cc"), ("Bcc", "bcc"), ("Subject", "subject")]
                .iter()
                .filter_map(|(name, k)| draft[k].as_str().filter(|v| !v.is_empty()).map(|v| serde_json::json!({ "name": name, "value": v })))
                .collect();
            let b64 = |v: &str| base64::engine::general_purpose::URL_SAFE.encode(v);
            let text = serde_json::json!({ "mimeType": "text/plain", "body": { "data": b64(draft["body"].as_str().unwrap_or_default()) } });
            // With an HTML part or an attachment, it is multipart, as Gmail
            // has it; an attachment comes by an id that differs on every
            // read, as Gmail's does.
            let mut parts = vec![];
            if let Some(html) = draft["html"].as_str() {
                parts.push(serde_json::json!({ "mimeType": "multipart/alternative", "parts": [text.clone(), { "mimeType": "text/html", "body": { "data": b64(html) } }] }));
            }
            if let Some(a) = draft.get("attachment").filter(|a| a.is_object()) {
                if parts.is_empty() {
                    parts.push(text.clone());
                }
                let size = a["data"].as_str().unwrap_or_default().len();
                parts.push(
                    serde_json::json!({ "mimeType": "application/pdf", "filename": a["filename"], "body": { "attachmentId": format!("att{}", before.len()), "size": size } }),
                );
            }
            let mut payload = if parts.is_empty() { text } else { serde_json::json!({ "mimeType": "multipart/mixed", "parts": parts }) };
            payload["headers"] = serde_json::json!(headers);
            ok(serde_json::json!({ "id": format!("d{n}"), "message": { "id": format!("dm{n}"), "threadId": "t1", "labelIds": ["DRAFT"], "payload": payload } }))
        }
        ("GET", p) if p.starts_with("/messages/dm") && p.contains("/attachments/att") => {
            use base64::Engine;
            let data = draft["attachment"]["data"].as_str().unwrap_or_default();
            ok(serde_json::json!({ "size": data.len(), "data": base64::engine::general_purpose::URL_SAFE.encode(data) }))
        }
        ("POST", "/drafts/send") => {
            let id = serde_json::from_str::<serde_json::Value>(&r.body).unwrap_or_default()["id"].as_str().unwrap_or_default().to_string();
            match id.strip_prefix('d').and_then(|n| n.parse::<usize>().ok()) {
                Some(n) if (1..=made).contains(&n) => ok(serde_json::json!({ "id": format!("s{n}"), "threadId": "t1", "labelIds": ["SENT"] })),
                _ => (404, String::new(), serde_json::json!({ "error": { "code": 404, "message": "Requested entity was not found." } }).to_string()),
            }
        }
        _ => (404, String::new(), serde_json::json!({ "error": { "code": 404, "message": "Requested entity was not found." } }).to_string()),
    }
}
