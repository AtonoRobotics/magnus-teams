//! Where the admin console can be reached from (admin console spec,
//! Access): only the organisation's network and VPN. It listens only on
//! configured internal addresses, never every address and never loopback;
//! it takes connections only from the allowed subnets, never from loopback,
//! so a tunnel misrouted to it (cloudflared reaches services over loopback)
//! is still refused; and it speaks only TLS 1.3. Each holds on its own if
//! another is misconfigured.

use anyhow::{bail, Context, Result};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;

/// A subnet: an address and how many leading bits name it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Subnet {
    addr: IpAddr,
    bits: u8,
}

impl Subnet {
    /// `192.168.1.0/24`, `fd00::/8`, or a single address.
    pub fn parse(s: &str) -> Result<Subnet> {
        let (a, b) = s.trim().split_once('/').unwrap_or((s.trim(), ""));
        let addr: IpAddr = a.parse().with_context(|| format!("{s:?} is not an address or subnet"))?;
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let bits = if b.is_empty() { max } else { b.parse().with_context(|| format!("{s:?}: the prefix length is not a number"))? };
        if bits > max {
            bail!("{s:?}: a prefix of {bits} bits is longer than the address");
        }
        Ok(Subnet { addr, bits })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = canonical(ip);
        match (self.addr, ip) {
            (IpAddr::V4(n), IpAddr::V4(a)) => prefix(&n.octets(), &a.octets(), self.bits),
            (IpAddr::V6(n), IpAddr::V6(a)) => prefix(&n.octets(), &a.octets(), self.bits),
            _ => false,
        }
    }

    /// Whether it takes in any loopback address.
    fn has_loopback(&self) -> bool {
        match self.addr {
            IpAddr::V4(_) => Subnet::parse("127.0.0.0/8").is_ok_and(|l| l.overlaps(self)),
            IpAddr::V6(_) => self.contains("::1".parse().unwrap()),
        }
    }

    fn overlaps(&self, other: &Subnet) -> bool {
        let shorter = if self.bits <= other.bits { self } else { other };
        let longer = if shorter == self { other } else { self };
        shorter.contains(longer.addr)
    }
}

/// An IPv4 address carried in IPv6 (`::ffff:a.b.c.d`) is that IPv4 address.
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        v4 => v4,
    }
}

fn prefix(net: &[u8], addr: &[u8], bits: u8) -> bool {
    let (whole, rest) = ((bits / 8) as usize, bits % 8);
    net[..whole] == addr[..whole] && (rest == 0 || (net[whole] ^ addr[whole]) >> (8 - rest) == 0)
}

/// The addresses to listen on, checked: never every address (`0.0.0.0`,
/// `::`) and never loopback.
pub fn listen_addrs(listen: &[String]) -> Result<Vec<SocketAddr>> {
    if listen.is_empty() {
        bail!("admin.listen names no address: give the server's address on the organisation's network");
    }
    listen
        .iter()
        .map(|l| {
            let a: SocketAddr = l.parse().with_context(|| format!("admin.listen: {l:?} is not an address and port"))?;
            let ip = canonical(a.ip());
            if ip.is_unspecified() {
                bail!("admin.listen: {l} would listen on every address; name the appliance's address on the organisation's network");
            }
            if ip.is_loopback() {
                bail!("admin.listen: {l} is loopback, which the tunnel reaches; the console listens only on the organisation's network");
            }
            Ok(a)
        })
        .collect()
}

/// The subnets connections may come from, checked: none takes in loopback.
pub fn allowlist(allow: &[String]) -> Result<Vec<Subnet>> {
    if allow.is_empty() {
        bail!("admin.allow names no subnet: give the organisation's network and VPN subnets");
    }
    allow
        .iter()
        .map(|a| {
            let s = Subnet::parse(a).with_context(|| "admin.allow".to_string())?;
            if s.has_loopback() {
                bail!("admin.allow: {a} takes in loopback, which the tunnel reaches; loopback is never allowed");
            }
            Ok(s)
        })
        .collect()
}

/// Whether a connection from `peer` may reach the console.
pub fn allowed(allow: &[Subnet], peer: IpAddr) -> bool {
    let peer = canonical(peer);
    !peer.is_loopback() && !peer.is_unspecified() && allow.iter().any(|s| s.contains(peer))
}

/// A certificate and its key, read from PEM files, checked to belong
/// together.
pub fn certified(cert: &std::path::Path, key: &std::path::Path) -> Result<Arc<rustls::sign::CertifiedKey>> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(cert)
        .with_context(|| format!("read {}", cert.display()))?
        .collect::<std::result::Result<_, _>>()
        .with_context(|| format!("read {}", cert.display()))?;
    anyhow::ensure!(!chain.is_empty(), "{} holds no certificate", cert.display());
    let key_der = PrivateKeyDer::from_pem_file(key).with_context(|| format!("read {}", key.display()))?;
    let k = rustls::sign::CertifiedKey::from_der(chain, key_der, &rustls::crypto::ring::default_provider())
        .with_context(|| format!("{} and {} do not make a certificate and its key", cert.display(), key.display()))?;
    Ok(Arc::new(k))
}

/// The certificates the console serves, by the name a browser asks for:
/// the trusted one (`admin.trusted_via`) for the console's hostname once it
/// is issued, the organisation's own for every other name. Either is
/// replaced while the console runs, for the next handshake.
#[derive(Debug)]
pub struct Certs {
    hostname: String,
    own: std::sync::RwLock<Arc<rustls::sign::CertifiedKey>>,
    trusted: std::sync::RwLock<Option<Arc<rustls::sign::CertifiedKey>>>,
}

impl Certs {
    pub fn new(hostname: &str, own: Arc<rustls::sign::CertifiedKey>) -> Arc<Certs> {
        Arc::new(Certs { hostname: hostname.to_ascii_lowercase(), own: std::sync::RwLock::new(own), trusted: std::sync::RwLock::new(None) })
    }

    pub fn set_own(&self, k: Arc<rustls::sign::CertifiedKey>) {
        *self.own.write().unwrap() = k;
    }

    pub fn set_trusted(&self, k: Arc<rustls::sign::CertifiedKey>) {
        *self.trusted.write().unwrap() = Some(k);
    }

    /// Whether the trusted certificate is being served.
    pub fn has_trusted(&self) -> bool {
        self.trusted.read().unwrap().is_some()
    }

    fn pick(&self, name: Option<&str>) -> Arc<rustls::sign::CertifiedKey> {
        if let Some(t) = self.trusted.read().unwrap().clone() {
            if name.is_some_and(|n| n.eq_ignore_ascii_case(&self.hostname)) {
                return t;
            }
        }
        self.own.read().unwrap().clone()
    }
}

impl rustls::server::ResolvesServerCert for Certs {
    fn resolve(&self, hello: rustls::server::ClientHello<'_>) -> Option<Arc<rustls::sign::CertifiedKey>> {
        Some(self.pick(hello.server_name()))
    }
}

/// TLS 1.3 only, with the certificates `certs` holds as each handshake
/// begins.
pub fn tls(certs: Arc<Certs>) -> Result<rustls::ServerConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut cfg = rustls::ServerConfig::builder_with_provider(provider).with_protocol_versions(&[&rustls::version::TLS13])?.with_no_client_auth().with_cert_resolver(certs);
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(cfg)
}

/// The console's listener: every configured address, each connection's
/// source checked before a byte of TLS is spoken, then the handshake.
pub struct Listener {
    rx: tokio::sync::mpsc::Receiver<(TlsStream<TcpStream>, SocketAddr)>,
    local: SocketAddr,
}

impl Listener {
    /// `allow` is shared: an admin's change to it (Operations › Network)
    /// takes effect at the next connection.
    pub async fn bind(addrs: &[SocketAddr], allow: Arc<std::sync::RwLock<Vec<Subnet>>>, cfg: rustls::ServerConfig) -> Result<Listener> {
        let acceptor = TlsAcceptor::from(Arc::new(cfg));
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let mut local = None;
        for a in addrs {
            let tcp = TcpListener::bind(a).await.with_context(|| format!("listen on {a}"))?;
            local.get_or_insert(tcp.local_addr()?);
            tracing::info!("magnus admin console listening on https://{}", tcp.local_addr()?);
            let (acceptor, allow, tx) = (acceptor.clone(), allow.clone(), tx.clone());
            tokio::spawn(async move {
                loop {
                    let (sock, peer) = match tcp.accept().await {
                        Ok(x) => x,
                        Err(e) => {
                            tracing::warn!("admin accept: {e}");
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            continue;
                        }
                    };
                    let ok = allowed(&allow.read().unwrap(), peer.ip());
                    if !ok {
                        tracing::warn!("admin console: refused a connection from {peer}, outside the allowed subnets");
                        drop(sock);
                        continue;
                    }
                    let (acceptor, tx) = (acceptor.clone(), tx.clone());
                    tokio::spawn(async move {
                        match tokio::time::timeout(Duration::from_secs(10), acceptor.accept(sock)).await {
                            Ok(Ok(s)) => {
                                let _ = tx.send((s, peer)).await;
                            }
                            Ok(Err(e)) => tracing::debug!("admin tls handshake with {peer}: {e}"),
                            Err(_) => tracing::debug!("admin tls handshake with {peer} timed out"),
                        }
                    });
                }
            });
        }
        Ok(Listener { rx, local: local.context("no address to listen on")? })
    }
}

impl axum::serve::Listener for Listener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            if let Some(x) = self.rx.recv().await {
                return x;
            }
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

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn a_subnet_takes_in_exactly_its_addresses() {
        let lan = Subnet::parse("192.168.1.0/24").unwrap();
        assert!(lan.contains(ip("192.168.1.7")) && lan.contains(ip("::ffff:192.168.1.7")));
        assert!(!lan.contains(ip("192.168.2.7")) && !lan.contains(ip("fd00::1")));
        let vpn = Subnet::parse("10.8.0.0/20").unwrap();
        assert!(vpn.contains(ip("10.8.15.255")) && !vpn.contains(ip("10.8.16.0")));
        assert!(Subnet::parse("fd00::/8").unwrap().contains(ip("fd12::1")));
        assert!(Subnet::parse("10.0.0.0/33").is_err() && Subnet::parse("lan").is_err());
    }

    #[test]
    fn loopback_and_every_address_are_never_the_consoles() {
        for bad in ["0.0.0.0:8443", "[::]:8443", "127.0.0.1:8443", "[::1]:8443", "127.5.5.5:8443"] {
            assert!(listen_addrs(&[bad.to_string()]).is_err(), "{bad}");
        }
        assert!(listen_addrs(&["192.168.1.10:8443".to_string()]).is_ok());
        assert!(listen_addrs(&[]).is_err());
        for bad in ["127.0.0.0/8", "127.0.0.1", "0.0.0.0/0", "::1", "::/0", "64.0.0.0/1", "128.0.0.0/1"] {
            let refused = allowlist(&[bad.to_string()]).is_err();
            assert_eq!(refused, bad != "128.0.0.0/1", "{bad}");
        }
        let allow = allowlist(&["192.168.1.0/24".to_string(), "10.8.0.0/24".to_string()]).unwrap();
        assert!(allowed(&allow, ip("10.8.0.5")) && !allowed(&allow, ip("10.9.0.5")));
        assert!(!allowed(&allow, ip("127.0.0.1")) && !allowed(&allow, ip("::1")));
    }

    fn key_for(name: &str) -> Arc<rustls::sign::CertifiedKey> {
        let c = rcgen::generate_simple_self_signed(vec![name.to_string()]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (cp, kp) = (dir.path().join("c.pem"), dir.path().join("k.pem"));
        std::fs::write(&cp, c.cert.pem()).unwrap();
        std::fs::write(&kp, c.key_pair.serialize_pem()).unwrap();
        certified(&cp, &kp).unwrap()
    }

    #[test]
    fn the_trusted_certificate_answers_for_the_hostname_alone_once_issued() {
        let (own, trusted) = (key_for("admin.acme.internal"), key_for("admin.tail1234.ts.net"));
        let certs = Certs::new("Admin.tail1234.ts.net", own.clone());
        assert!(Arc::ptr_eq(&certs.pick(Some("admin.tail1234.ts.net")), &own), "nothing issued yet: the organisation's own");
        certs.set_trusted(trusted.clone());
        assert!(certs.has_trusted());
        assert!(Arc::ptr_eq(&certs.pick(Some("ADMIN.tail1234.ts.net")), &trusted));
        for other in [Some("192.168.1.10"), Some("admin.local"), None] {
            assert!(Arc::ptr_eq(&certs.pick(other), &own), "{other:?}");
        }
        let renewed = key_for("admin.acme.internal");
        certs.set_own(renewed.clone());
        assert!(Arc::ptr_eq(&certs.pick(None), &renewed), "a renewed certificate is served at the next handshake");
    }

    #[test]
    fn a_key_that_is_not_the_certificates_is_refused() {
        let (a, b) = (rcgen::generate_simple_self_signed(vec!["a".into()]).unwrap(), rcgen::generate_simple_self_signed(vec!["b".into()]).unwrap());
        let dir = tempfile::tempdir().unwrap();
        let (cp, kp) = (dir.path().join("c.pem"), dir.path().join("k.pem"));
        std::fs::write(&cp, a.cert.pem()).unwrap();
        std::fs::write(&kp, b.key_pair.serialize_pem()).unwrap();
        assert!(certified(&cp, &kp).is_err());
        assert!(certified(&dir.path().join("missing.pem"), &kp).is_err());
    }
}
