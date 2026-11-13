//! HTTPS for people's apps on the network.
//!
//! The owner can name a certificate and key. Otherwise the appliance makes a
//! self-signed certificate on first start and keeps it; apps pin its SHA-256
//! fingerprint on their first connection, as SSH pins a host key.

use anyhow::{Context, Result};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;

/// The pause after a failed accept before the next.
const ACCEPT_RETRY: Duration = Duration::from_millis(100);
/// How long a new connection has to send its first byte, then to finish the TLS handshake.
const HANDSHAKE_WAIT: Duration = Duration::from_secs(10);
/// How long, and how much, a plain-http request is read before it is sent to https.
const PLAIN_HEAD_WAIT: Duration = Duration::from_secs(10);
const PLAIN_HEAD_MOST_BYTES: usize = 8192;

/// The server configuration and the fingerprint of its certificate.
pub fn load(state_dir: &Path, cert: Option<&Path>, key: Option<&Path>) -> Result<(rustls::ServerConfig, String)> {
    let (cert_path, key_path) = match (cert, key) {
        (Some(c), Some(k)) => (c.to_path_buf(), k.to_path_buf()),
        (None, None) => {
            let dir = state_dir.join("tls");
            std::fs::create_dir_all(&dir)?;
            let (c, k) = (dir.join("cert.pem"), dir.join("key.pem"));
            if !c.exists() || !k.exists() {
                let host = hostname();
                let mut names = vec!["localhost".to_string(), "magnus".to_string()];
                if !host.is_empty() {
                    names.push(host);
                }
                let generated = rcgen::generate_simple_self_signed(names).context("make the server certificate")?;
                crate::vault::write_private(&k, generated.key_pair.serialize_pem().as_bytes())?;
                std::fs::write(&c, generated.cert.pem())?;
            }
            (c, k)
        }
        _ => anyhow::bail!("set both server.tls_cert and server.tls_key, or neither"),
    };
    let chain: Vec<CertificateDer<'static>> =
        CertificateDer::pem_file_iter(&cert_path).with_context(|| format!("read {}", cert_path.display()))?.collect::<std::result::Result<_, _>>()?;
    anyhow::ensure!(!chain.is_empty(), "{} holds no certificate", cert_path.display());
    let key = PrivateKeyDer::from_pem_file(&key_path).with_context(|| format!("read {}", key_path.display()))?;
    let fingerprint = fingerprint(&chain[0]);
    let mut cfg = rustls::ServerConfig::builder().with_no_client_auth().with_single_cert(chain, key)?;
    // HTTP/1.1 only: the server speaks nothing else (axum's `http2` is off),
    // and a client that agreed to h2 got its connection closed.
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok((cfg, fingerprint))
}

/// SHA-256 of the certificate, as colon-separated hex pairs.
pub fn fingerprint(cert: &CertificateDer<'_>) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(cert.as_ref()).iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()).unwrap_or_default()
}

/// A TCP listener that completes TLS handshakes off the accept path, so one
/// slow client cannot hold up the others.
pub struct Listener {
    rx: tokio::sync::mpsc::Receiver<(TlsStream<TcpStream>, SocketAddr)>,
    local: SocketAddr,
}

impl Listener {
    pub async fn bind(addr: &str, cfg: rustls::ServerConfig) -> Result<Listener> {
        let tcp = TcpListener::bind(addr).await.with_context(|| format!("listen on {addr}"))?;
        let local = tcp.local_addr()?;
        let acceptor = TlsAcceptor::from(Arc::new(cfg));
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        tokio::spawn(async move {
            loop {
                let (sock, peer) = match tcp.accept().await {
                    Ok(x) => x,
                    Err(e) => {
                        tracing::warn!("tls accept: {e}");
                        tokio::time::sleep(ACCEPT_RETRY).await;
                        continue;
                    }
                };
                let (acceptor, tx) = (acceptor.clone(), tx.clone());
                tokio::spawn(async move {
                    // A TLS connection starts with a handshake record (0x16).
                    // Anything else is a browser that was given the address
                    // without https://: send it there instead of TLS bytes.
                    let mut first = [0u8; 1];
                    match tokio::time::timeout(HANDSHAKE_WAIT, sock.peek(&mut first)).await {
                        Ok(Ok(1)) if first[0] != 0x16 => {
                            let here = sock.local_addr().unwrap_or(local);
                            return redirect_to_https(sock, here).await;
                        }
                        Ok(Ok(1)) => {}
                        _ => return,
                    }
                    match tokio::time::timeout(HANDSHAKE_WAIT, acceptor.accept(sock)).await {
                        Ok(Ok(s)) => {
                            let _ = tx.send((s, peer)).await;
                        }
                        Ok(Err(e)) => tracing::debug!("tls handshake with {peer}: {e}"),
                        Err(_) => tracing::debug!("tls handshake with {peer} timed out"),
                    }
                });
            }
        });
        Ok(Listener { rx, local })
    }
}

/// Answer a plain HTTP request on the HTTPS port with a redirect to the
/// same address over https. The host comes from the request (the address
/// the person typed) when it looks like a host, else the address the
/// connection came in on.
async fn redirect_to_https(mut sock: TcpStream, local: SocketAddr) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    let read = async {
        while head.len() < PLAIN_HEAD_MOST_BYTES && !head.windows(4).any(|w| w == b"\r\n\r\n") {
            match sock.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => head.extend_from_slice(&buf[..n]),
            }
        }
    };
    let _ = tokio::time::timeout(PLAIN_HEAD_WAIT, read).await;
    let location = https_location(&String::from_utf8_lossy(&head), local);
    let reply = format!("HTTP/1.1 308 Permanent Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let _ = sock.write_all(reply.as_bytes()).await;
    let _ = sock.shutdown().await;
}

fn https_location(head: &str, local: SocketAddr) -> String {
    let mut lines = head.split("\r\n");
    let path = lines.next().and_then(|l| l.split(' ').nth(1)).filter(|p| p.starts_with('/') && !p.starts_with("//")).unwrap_or("/");
    let host = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("host"))
        .map(|(_, v)| v.trim().to_string())
        .filter(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(c)))
        .unwrap_or_else(|| local.to_string());
    let path: String = path.chars().filter(|c| c.is_ascii_graphic()).collect();
    format!("https://{host}{path}")
}

impl axum::serve::Listener for Listener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            if let Some(x) = self.rx.recv().await {
                return x;
            }
            // The accept task never ends while the listener lives.
            std::future::pending::<()>().await;
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        Ok(self.local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_is_sent_to_the_same_address_over_https() {
        let local: SocketAddr = "0.0.0.0:7700".parse().unwrap();
        assert_eq!(https_location("GET /#/inbox HTTP/1.1\r\nHost: 192.168.0.50:7700\r\n\r\n", local), "https://192.168.0.50:7700/#/inbox");
        assert_eq!(https_location("GET / HTTP/1.1\r\nhost: magnus.local:7700\r\n\r\n", local), "https://magnus.local:7700/");
        // No usable host: the listener's own address; no way to point elsewhere.
        assert_eq!(https_location("GET //evil.example/x HTTP/1.1\r\nHost: a b\r\n\r\n", local), "https://0.0.0.0:7700/");
        assert_eq!(https_location("", local), "https://0.0.0.0:7700/");
    }
}
