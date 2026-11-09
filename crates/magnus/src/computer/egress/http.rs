//! One connection from a computer: HTTP/1.1, plain or inside a CONNECT whose
//! TLS the proxy ends itself. Every request is decided, recorded and only
//! then sent, on a new upstream connection pinned to the address checked.

use super::audit::Row;
use super::{ca, policy, surrogate, Proxy};
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, UnixStream};

const HEAD_WAIT: Duration = Duration::from_secs(30);
const UPSTREAM_WAIT: Duration = Duration::from_secs(300);
const MAX_HEAD_BYTES: usize = 64 << 10;
const MAX_BODY_BYTES: u64 = 256 << 20;
const MAX_PAGE_BYTES: usize = 2 << 20;
const COPY_BUFFER_BYTES: usize = 64 << 10;
const HOP_BY_HOP: [&str; 8] = ["connection", "proxy-connection", "proxy-authorization", "keep-alive", "te", "trailer", "transfer-encoding", "upgrade"];

pub(super) struct Head {
    pub(super) method: String,
    pub(super) target: String,
    pub(super) headers: Vec<(String, String)>,
}

impl Head {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

async fn read_head(from: &mut (impl AsyncBufRead + Unpin)) -> Result<Option<Head>> {
    let mut raw = Vec::new();
    let read = async {
        loop {
            let before = raw.len();
            if from.read_until(b'\n', &mut raw).await? == 0 {
                return Ok::<bool, anyhow::Error>(!raw.is_empty());
            }
            anyhow::ensure!(raw.len() <= MAX_HEAD_BYTES, "a request head is too long");
            if raw[before..] == *b"\r\n" || raw[before..] == *b"\n" {
                return Ok(true);
            }
        }
    };
    if !tokio::time::timeout(HEAD_WAIT, read).await.context("a request head took too long")?? {
        return Ok(None);
    }
    let text = String::from_utf8(raw).context("a request head is not text")?;
    let mut lines = text.split("\r\n").flat_map(|l| l.split('\n'));
    let mut first = lines.next().unwrap_or_default().split(' ');
    let (Some(method), Some(target), Some(_)) = (first.next(), first.next(), first.next()) else { bail!("not an HTTP request") };
    let headers = lines.filter_map(|l| l.split_once(':')).map(|(n, v)| (n.trim().to_string(), v.trim().to_string())).collect();
    Ok(Some(Head { method: method.to_string(), target: target.to_string(), headers }))
}

async fn respond(to: &mut (impl AsyncWrite + Unpin), status: &str, extra: &str, body: &str) -> Result<()> {
    to.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}", body.len()).as_bytes()).await?;
    to.flush().await?;
    Ok(())
}

fn host_and_port(authority: &str, default_port: u16) -> Option<(String, u16)> {
    match authority.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(']') || p.parse::<u16>().is_ok() => Some((h.trim_matches(['[', ']']).to_string(), p.parse().ok()?)),
        _ => Some((authority.trim_matches(['[', ']']).to_string(), default_port)),
    }
}

pub(super) async fn serve_connection(proxy: Arc<Proxy>, computer: String, conn: UnixStream) -> Result<()> {
    let mut stream = BufReader::new(conn);
    let Some(first) = read_head(&mut stream).await? else { return Ok(()) };
    if first.method == "CONNECT" {
        let Some((host, port)) = host_and_port(&first.target, 443) else { bail!("a bad CONNECT target") };
        stream.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").await?;
        stream.flush().await?;
        // The CA is unavailable while the vault is sealed: nothing leaves then.
        let tls = ca::server_config(&proxy.kernel.vault, &computer, &host)?;
        let accepted = tokio::time::timeout(HEAD_WAIT, tokio_rustls::TlsAcceptor::from(tls).accept(stream)).await.context("a TLS handshake took too long")??;
        let mut inner = BufReader::new(accepted);
        let Some(head) = read_head(&mut inner).await? else { return Ok(()) };
        return request(&proxy, &computer, &mut inner, head, &host, port, true).await;
    }
    let Ok(url) = url::Url::parse(&first.target) else {
        return respond(&mut stream, "400 Bad Request", "", "a forward proxy takes an absolute URL\n").await;
    };
    let (Some(host), "http") = (url.host_str().map(|h| h.trim_matches(['[', ']']).to_string()), url.scheme()) else {
        return respond(&mut stream, "400 Bad Request", "", "only http here; https goes through CONNECT\n").await;
    };
    let port = url.port_or_known_default().unwrap_or(80);
    request(&proxy, &computer, &mut stream, Head { target: url[url::Position::BeforePath..].to_string(), ..first }, &host, port, false).await
}

fn audited_path(k: &crate::kernel::Kernel, computer: &str, target: &str) -> String {
    let identity = k.read(|s| s.computers.get(computer).is_some_and(|c| matches!(c.role, crate::computer::Role::Identity { .. })));
    if identity {
        target.split('?').next().unwrap_or_default().to_string()
    } else {
        target.to_string()
    }
}

async fn request<S: AsyncRead + AsyncWrite + Unpin>(proxy: &Proxy, computer: &str, stream: &mut BufReader<S>, head: Head, host: &str, port: u16, tls: bool) -> Result<()> {
    // A form can put a secret in a URL. An identity's browser logs the path only.
    let audited = audited_path(&proxy.kernel, computer, &head.target);
    let row = |phase, address, detail| Row { computer, phase, host, port, method: &head.method, path: &audited, address, detail };
    let decided = proxy.kernel.read(|s| match s.computers.get(computer) {
        Some(c) => {
            let learned = proxy.learned.lock().unwrap_or_else(|e| e.into_inner());
            let registries = proxy.kernel.cfg.computer.as_ref().map(|c| c.registries.clone()).unwrap_or_default();
            policy::classify(&registries, c, &learned, host, port, &head.method).map(String::from)
        }
        None => Err("the computer is unknown".to_string()),
    });
    let why = match decided {
        Ok(why) => why,
        Err(reason) => return refuse(proxy, stream.get_mut(), row("refused", None, &reason), &reason).await,
    };
    let (head, scrubber) = match surrogate::swapped(&proxy.kernel, computer, head, host).await {
        Ok(h) => h,
        Err((head, reason)) => {
            let row = Row { computer, phase: "refused", host, port, method: &head.method, path: &audited, address: None, detail: &reason };
            return refuse(proxy, stream.get_mut(), row, &reason).await;
        }
    };
    let row = |phase, address, detail| Row { computer, phase, host, port, method: &head.method, path: &audited, address, detail };
    let address = match proxy.resolve(host, port).await {
        Ok(a) => a,
        Err(reason) => return refuse(proxy, stream.get_mut(), row("refused", None, &reason), &reason).await,
    };
    let shown = address.to_string();
    // No row, no packet.
    if let Err(e) = proxy.audit.write(row("intent", Some(&shown), &why)).await {
        tracing::error!("egress of computer {computer} not sent, its record failed: {e:#}");
        return respond(stream.get_mut(), "502 Bad Gateway", "", "the request could not be recorded, so it was not sent\n").await;
    }
    let result = forward(proxy, computer, stream, &head, Upstream { host, address, tls }, scrubber).await;
    let detail = match &result {
        Ok(status) => status.clone(),
        Err(e) => format!("{e:#}"),
    };
    if let Err(e) = proxy.audit.write(row("result", Some(&shown), &detail)).await {
        tracing::warn!("computer {computer}'s egress result was not recorded: {e:#}");
    }
    if result.is_err() {
        respond(stream.get_mut(), "502 Bad Gateway", "", "the destination could not be reached\n").await?;
    }
    Ok(())
}

/// A refusal is sent at once, and carries the id of its row.
async fn refuse(proxy: &Proxy, to: &mut (impl AsyncWrite + Unpin), row: Row<'_>, reason: &str) -> Result<()> {
    match proxy.audit.write(row).await {
        Ok(id) => respond(to, "403 Forbidden", &format!("X-Magnus-Refused: {id}\r\n"), &format!("{reason}\n")).await,
        Err(e) => {
            tracing::error!("a refused egress could not be recorded: {e:#}");
            respond(to, "403 Forbidden", "", &format!("{reason}\n")).await
        }
    }
}

trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

struct Upstream<'a> {
    host: &'a str,
    address: SocketAddr,
    tls: bool,
}

/// Sends one request and streams back the answer. Says the status line.
async fn forward<S: AsyncRead + AsyncWrite + Unpin>(
    proxy: &Proxy,
    computer: &str,
    stream: &mut BufReader<S>,
    head: &Head,
    to: Upstream<'_>,
    mut scrubber: surrogate::Scrubber,
) -> Result<String> {
    let Upstream { host, address, tls } = to;
    let length: u64 = head.header("content-length").map(str::parse).transpose().context("a bad Content-Length")?.unwrap_or(0);
    anyhow::ensure!(length <= MAX_BODY_BYTES, "a request body is too long");
    anyhow::ensure!(!head.header("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked")), "a chunked request body is not carried");
    let tcp = tokio::time::timeout(UPSTREAM_WAIT, TcpStream::connect(address)).await.context("connecting took too long")??;
    let mut upstream: Box<dyn Stream> = if tls {
        let name = rustls::pki_types::ServerName::try_from(host.to_string()).context("a bad host name")?;
        Box::new(tokio_rustls::TlsConnector::from(proxy.client_tls.clone()).connect(name, tcp).await.context("the destination's TLS")?)
    } else {
        Box::new(tcp)
    };
    let page = proxy.kernel.read(|s| s.computers.get(computer).and_then(|c| c.pages.iter().find(|p| p.host.eq_ignore_ascii_case(host)).map(|p| p.id.clone())));
    let mut out = format!("{} {} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n", head.method, head.target);
    for (name, value) in &head.headers {
        let lower = name.to_ascii_lowercase();
        // Plain text, so a page's markup can be read for what it loads.
        // A compressed answer could repeat a credential where it cannot be seen.
        if lower == "host" || HOP_BY_HOP.contains(&lower.as_str()) || ((page.is_some() || scrubber.active()) && lower == "accept-encoding") {
            continue;
        }
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str("\r\n");
    upstream.write_all(out.as_bytes()).await?;
    if length > 0 {
        let mut body = (&mut *stream).take(length);
        tokio::io::copy(&mut body, &mut upstream).await?;
    }
    upstream.flush().await?;

    let mut from_upstream = BufReader::new(upstream);
    let mut response_head = Vec::new();
    tokio::time::timeout(HEAD_WAIT, async {
        loop {
            let before = response_head.len();
            if from_upstream.read_until(b'\n', &mut response_head).await? == 0 || response_head[before..] == *b"\r\n" {
                return Ok::<(), anyhow::Error>(());
            }
            anyhow::ensure!(response_head.len() <= MAX_HEAD_BYTES, "a response head is too long");
        }
    })
    .await
    .context("the destination took too long to answer")??;
    let text = String::from_utf8_lossy(&response_head).into_owned();
    let status = text.lines().next().unwrap_or_default().to_string();
    let is_html = text.lines().any(|l| l.to_ascii_lowercase().starts_with("content-type:") && l.to_ascii_lowercase().contains("html"));
    // Putting a surrogate back changes the length; the closing connection ends it.
    let keep = |l: &&str| !l.to_ascii_lowercase().starts_with("connection:") && !(scrubber.active() && l.to_ascii_lowercase().starts_with("content-length:"));
    let without_connection: String = scrubber.text(&text.split_inclusive('\n').filter(keep).collect::<String>());
    let header_end = without_connection.len().saturating_sub(2);
    stream.get_mut().write_all(format!("{}Connection: close\r\n\r\n", &without_connection[..header_end]).as_bytes()).await?;

    let mut seen = Vec::new();
    let mut download = (head.method == "GET" && status.split_whitespace().nth(1) == Some("200")).then(Sha256::new);
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    loop {
        let n = tokio::time::timeout(UPSTREAM_WAIT, from_upstream.read(&mut buffer)).await.context("the destination stopped answering")??;
        if n == 0 {
            break;
        }
        if let Some(hash) = &mut download {
            hash.update(&buffer[..n]);
        }
        if is_html && page.is_some() && seen.len() < MAX_PAGE_BYTES {
            seen.extend_from_slice(&buffer[..n]);
        }
        let clean = scrubber.push(&buffer[..n]);
        stream.get_mut().write_all(&clean).await?;
    }
    let rest = scrubber.finish();
    stream.get_mut().write_all(&rest).await?;
    stream.get_mut().flush().await?;
    if let Some(hash) = download {
        super::saw_download(computer, host, hex::encode(hash.finalize()));
    }
    if let Some(page) = page {
        if !seen.is_empty() {
            let hosts = policy::resources_of(&String::from_utf8_lossy(&seen));
            proxy.learned.lock().unwrap_or_else(|e| e.into_inner()).entry(page).or_default().extend(hosts);
        }
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_head_parses_and_a_bad_one_does_not() {
        let mut ok = BufReader::new(&b"GET http://a.test/x HTTP/1.1\r\nHost: a.test\r\nX-Y: z\r\n\r\nrest"[..]);
        let head = read_head(&mut ok).await.unwrap().unwrap();
        assert_eq!((head.method.as_str(), head.target.as_str(), head.header("x-y")), ("GET", "http://a.test/x", Some("z")));
        let mut bad = BufReader::new(&b"nonsense\r\n\r\n"[..]);
        assert!(read_head(&mut bad).await.is_err());
    }

    #[test]
    fn a_target_gives_its_host_and_port() {
        assert_eq!(host_and_port("a.test:8443", 443), Some(("a.test".to_string(), 8443)));
        assert_eq!(host_and_port("a.test", 443), Some(("a.test".to_string(), 443)));
        assert_eq!(host_and_port("[::1]:8080", 443), Some(("::1".to_string(), 8080)));
    }
}
