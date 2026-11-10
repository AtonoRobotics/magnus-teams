//! What every brokered connector's server shares (connectors spec, Brokered
//! API connectors). The server is the image's `/init`, alone in its own
//! Firecracker microVM, which has no network device and no disk: it answers
//! MCP on vsock, one session per connection, with newline-delimited
//! JSON-RPC (MCP's stdio framing, carried on vsock), and reaches its
//! provider only through the appliance's egress proxy, by HTTPS CONNECT to
//! the host on vsock, and only over TLS whose certificate it has checked
//! for the provider's name (`https`), so no token leaves it in the clear,
//! not even to the proxy. The access token comes with each call
//! (`params._meta["io.magnus/access-token"]`) and is used for that call
//! alone: nothing is kept between calls and nothing is written anywhere.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, OnceLock};

pub mod schema;

/// The vsock port the server answers MCP on.
pub const MCP_PORT: u32 = 5252;
/// The port on the host (vsock CID 2) where the appliance's egress proxy
/// answers this microVM, and nothing else does.
pub const PROXY_PORT: u32 = 3128;
/// Where the appliance puts the access token in each call.
pub const TOKEN_META: &str = "io.magnus/access-token";
pub const PROTOCOL: &str = "2025-06-18";
/// The most a proxy's answer to CONNECT may be (its status line and headers).
const HEAD_MAX: usize = 8192;

/// Where a failed call's result says what it means for what happened at the
/// provider (`unknown`, `needs_scope` or `bad_answer`; nothing said,
/// nothing happened).
pub const OUTCOME_META: &str = "io.magnus/outcome";
/// The outcome of a call the tool made, whose answer does not match its
/// output schema: what the provider did is unknown (#193). The appliance
/// knows which tools change things, so it decides what that means: a
/// write that may have happened, or a read that came back unusable.
pub const BAD_ANSWER: &str = "bad_answer";
/// With `needs_scope`, the scopes the provider wants.
pub const SCOPES_META: &str = "io.magnus/scopes";

/// Why a call has no answer, in words for the agent, and what happened at
/// the provider.
#[derive(Debug, PartialEq)]
pub enum Failure {
    /// Nothing happened there.
    NotDone(String),
    /// The request may have reached it; what it did is unknown.
    Unknown(String),
    /// It refused for want of access the sign-in does not hold: the scopes
    /// it wants. Nothing happened there.
    NeedsScope(String, Vec<String>),
}

impl From<String> for Failure {
    fn from(s: String) -> Self {
        Failure::NotDone(s)
    }
}

/// One tool: its schemas and what it does with its input and the call's
/// access token.
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub input: Value,
    pub output: Value,
    pub run: fn(&Value, &str) -> Result<Value, Failure>,
}

/// The answer to one message, or none for a notification.
pub fn answer(msg: &Value, server: &str, tools: &[Tool]) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let result = match msg["method"].as_str().unwrap_or_default() {
        "initialize" => json!({ "protocolVersion": PROTOCOL, "capabilities": { "tools": {} }, "serverInfo": { "name": server, "version": env!("CARGO_PKG_VERSION") } }),
        "tools/list" => {
            json!({ "tools": tools.iter().map(|t| json!({ "name": t.name, "description": t.description, "inputSchema": t.input, "outputSchema": t.output })).collect::<Vec<_>>() })
        }
        "tools/call" => call(&msg["params"], tools),
        other => return Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("no method {other}") } })),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn failed(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": true })
}

fn call(params: &Value, tools: &[Tool]) -> Value {
    let name = params["name"].as_str().unwrap_or_default();
    let Some(t) = tools.iter().find(|t| t.name == name) else { return failed(format!("no tool {name}")) };
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    if let Err(e) = schema::check(&t.input, &args) {
        return failed(format!("{name}: the input does not match its schema: {e}"));
    }
    let Some(token) = params["_meta"][TOKEN_META].as_str().filter(|t| !t.is_empty()) else {
        return failed(format!("{name}: no access token came with this call"));
    };
    match (t.run)(&args, token) {
        Ok(v) => match schema::check(&t.output, &v) {
            Ok(()) => json!({ "content": [{ "type": "text", "text": v.to_string() }], "structuredContent": v, "isError": false }),
            Err(e) => with_meta(failed(format!("{name}: the answer does not match its schema: {e}")), json!({ OUTCOME_META: BAD_ANSWER })),
        },
        Err(Failure::NotDone(e)) => failed(e),
        Err(Failure::Unknown(e)) => with_meta(failed(e), json!({ OUTCOME_META: "unknown" })),
        Err(Failure::NeedsScope(e, scopes)) => with_meta(failed(e), json!({ OUTCOME_META: "needs_scope", SCOPES_META: scopes })),
    }
}

fn with_meta(mut v: Value, meta: Value) -> Value {
    v["_meta"] = meta;
    v
}

/// One session: answer each line until the appliance closes it.
pub fn session(r: impl Read, mut w: impl Write, server: &str, tools: &[Tool]) {
    for line in BufReader::new(r).lines() {
        let Ok(line) = line else { return };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(a) = answer(&msg, server, tools) {
            if writeln!(w, "{a}").and_then(|_| w.flush()).is_err() {
                return;
            }
        }
    }
}

fn vsock(cid: u32, port: u32, listen: bool) -> std::io::Result<UnixStream> {
    unsafe {
        let fd = libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut addr: libc::sockaddr_vm = std::mem::zeroed();
        addr.svm_family = libc::AF_VSOCK as libc::sa_family_t;
        addr.svm_port = port;
        addr.svm_cid = cid;
        let (a, n) = (&addr as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_vm>() as u32);
        let rc = if listen {
            if libc::bind(fd, a, n) == 0 {
                libc::listen(fd, 64)
            } else {
                -1
            }
        } else {
            libc::connect(fd, a, n)
        };
        if rc != 0 {
            let e = std::io::Error::last_os_error();
            libc::close(fd);
            return Err(e);
        }
        // Any stream socket reads and writes as one.
        Ok(UnixStream::from_raw_fd(fd))
    }
}

/// As the microVM's PID 1: answer MCP on vsock, a thread per session.
pub fn serve(server: &'static str, tools: Vec<Tool>) -> ! {
    let tools: &'static [Tool] = Box::leak(tools.into_boxed_slice());
    let l = vsock(libc::VMADDR_CID_ANY, MCP_PORT, true).unwrap_or_else(|e| panic!("vsock listen: {e}"));
    loop {
        let fd = unsafe { libc::accept4(std::os::fd::AsRawFd::as_raw_fd(&l), std::ptr::null_mut(), std::ptr::null_mut(), libc::SOCK_CLOEXEC) };
        if fd < 0 {
            continue;
        }
        let s = unsafe { UnixStream::from_raw_fd(fd) };
        std::thread::spawn(move || {
            let Ok(w) = s.try_clone() else { return };
            session(s, w, server, tools)
        });
    }
}

/// A connection to `target` (`host:port`) through the appliance's egress
/// proxy, which admits only the hosts in the connector's manifest: the
/// proxy's refusal when it does not. Only TLS is carried on it (`https`).
fn tunnel(target: &str) -> Result<UnixStream, String> {
    let mut s = vsock(libc::VMADDR_CID_HOST, PROXY_PORT, false).map_err(|e| format!("could not reach the appliance's proxy: {e}"))?;
    write!(s, "CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n").map_err(|e| e.to_string())?;
    let mut head = vec![];
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match s.read(&mut b) {
            Ok(1) if head.len() < HEAD_MAX => head.push(b[0]),
            _ => return Err(format!("the proxy closed the connection to {target}")),
        }
    }
    let head = String::from_utf8_lossy(&head);
    let status = head.lines().next().unwrap_or_default().to_string();
    match status.split_whitespace().nth(1) {
        Some("200") => Ok(s),
        _ => Err(format!("the proxy refused {target}: {status}")),
    }
}

/// The way out to a provider: TLS over the proxy's tunnel, its handshake
/// done and the provider's certificate checked for its name.
pub type Https = rustls::StreamOwned<rustls::ClientConnection, UnixStream>;

/// Where the appliance names roots to trust beside the public ones (an
/// organisation whose egress inspects TLS, or a test's stand-in): the
/// kernel hands a `key=value` from its command line that it does not know
/// to `/init` as its environment. Base64url DER certificates, separated by
/// commas.
pub const ROOTS_ENV: &str = "magnus_roots";

/// The TLS client: the public roots (webpki-roots), and those the appliance
/// names.
fn tls() -> Arc<rustls::ClientConfig> {
    static C: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    C.get_or_init(|| {
        use base64::Engine;
        let more = std::env::var(ROOTS_ENV).unwrap_or_default();
        let more = more.split(',').filter_map(|d| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(d).ok());
        client(more.collect())
    })
    .clone()
}

fn client(more: Vec<Vec<u8>>) -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    for der in more {
        let _ = roots.add(rustls::pki_types::CertificateDer::from(der));
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Arc::new(rustls::ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions().expect("TLS versions").with_root_certificates(roots).with_no_client_auth())
}

/// A connection to the provider at `target` (`host:port`): through the
/// proxy, then TLS, and nothing is sent on it until the provider has shown
/// a certificate for `host` from a trusted root. A failure here means
/// nothing reached the provider.
pub fn https(target: &str) -> Result<Https, String> {
    let host = target.rsplit_once(':').map_or(target, |(h, _)| h);
    secure(tunnel(target)?, host, tls())
}

fn secure<S: Read + Write>(sock: S, host: &str, config: Arc<rustls::ClientConfig>) -> Result<rustls::StreamOwned<rustls::ClientConnection, S>, String> {
    let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| format!("{host} is not a name to reach securely: {e}"))?;
    let conn = rustls::ClientConnection::new(config, name).map_err(|e| e.to_string())?;
    let mut s = rustls::StreamOwned::new(conn, sock);
    while s.conn.is_handshaking() {
        s.conn.complete_io(&mut s.sock).map_err(|e| format!("could not reach {host} securely: {e}"))?;
    }
    Ok(s)
}

/// An HTTP response: its status, its headers as sent, and its body.
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub head: String,
    pub body: Vec<u8>,
}

impl Response {
    /// A header's value, by its name in any case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().skip(1).find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim()))
    }
}

/// One HTTP/1.1 request to the provider over `s`, which is closed after it.
/// An answer cut short is an error, never a response.
pub fn exchange(s: Https, method: &str, host: &str, path: &str, token: &str, body: Option<&Value>) -> Result<Response, String> {
    exchange_over(s, method, host, path, token, body)
}

fn exchange_over(s: impl Read + Write, method: &str, host: &str, path: &str, token: &str, body: Option<&Value>) -> Result<Response, String> {
    let data = body.map(|b| b.to_string());
    send_over(s, method, host, path, token, "application/json", data.as_deref().map(|d| Body { kind: "application/json", bytes: d.as_bytes() }))
}

/// A request's body: its media type and its bytes as sent.
pub struct Body<'a> {
    pub kind: &'a str,
    pub bytes: &'a [u8],
}

/// One HTTP/1.1 request to the provider over `s` with a body of any type
/// (a file's bytes), and an answer of the type `accept` names: as
/// [`exchange`] otherwise.
pub fn send(s: Https, method: &str, host: &str, path: &str, token: &str, accept: &str, body: Option<Body>) -> Result<Response, String> {
    send_over(s, method, host, path, token, accept, body)
}

fn send_over(mut s: impl Read + Write, method: &str, host: &str, path: &str, token: &str, accept: &str, body: Option<Body>) -> Result<Response, String> {
    let n = body.as_ref().map_or(0, |b| b.bytes.len());
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {token}\r\nAccept: {accept}\r\nConnection: close\r\nContent-Length: {n}\r\n");
    if let Some(b) = &body {
        req.push_str(&format!("Content-Type: {}\r\n", b.kind));
    }
    req.push_str("\r\n");
    let mut out = req.into_bytes();
    out.extend_from_slice(body.map_or(&[][..], |b| b.bytes));
    s.write_all(&out).and_then(|_| s.flush()).map_err(|e| format!("sending to {host}: {e}"))?;
    let mut raw = vec![];
    // A TLS peer may end without its closing alert (rustls says so as an
    // unexpected end); that is an end only where the answer says it is
    // whole, by its length or its last chunk.
    let ended = match s.read_to_end(&mut raw) {
        Ok(_) => None,
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Some(e),
        Err(e) => return Err(format!("reading {host}'s answer: {e}")),
    };
    let cut = || format!("{host}'s answer was cut short");
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| format!("{host} gave no answer"))?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let rest = &raw[split + 4..];
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| format!("{host} gave no status"))?;
    let r = Response { status, body: vec![], head };
    let body = if status < 200 || status == 204 || status == 304 {
        // These have no body, whatever they say (RFC 9112, 6.3).
        vec![]
    } else if r.header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        dechunk(rest).ok_or_else(cut)?
    } else if let Some(n) = r.header("content-length") {
        let n: usize = n.parse().map_err(|_| format!("{host} gave a length that is not one: {n}"))?;
        rest.get(..n).ok_or_else(cut)?.to_vec()
    } else if ended.is_some() {
        // Delimited only by the end, which did not come as one.
        return Err(cut());
    } else {
        rest.to_vec()
    };
    Ok(Response { body, ..r })
}

/// A chunked body joined, or none unless it ends with its last chunk.
fn dechunk(mut b: &[u8]) -> Option<Vec<u8>> {
    let mut out = vec![];
    loop {
        let eol = b.windows(2).position(|w| w == b"\r\n")?;
        let size = usize::from_str_radix(std::str::from_utf8(&b[..eol]).ok()?.split(';').next()?.trim(), 16).ok()?;
        if size == 0 {
            return Some(out);
        }
        let data = b.get(eol + 2..eol + 2 + size)?;
        if b.get(eol + 2 + size..eol + 4 + size)? != b"\r\n" {
            return None;
        }
        out.extend_from_slice(data);
        b = &b[eol + 4 + size..];
    }
}

#[cfg(test)]
#[path = "../../../tests/brokered-kit/kit.rs"]
mod tests;
