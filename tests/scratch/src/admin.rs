//! The admin console as tests run it: `magnus-admin` started as its own
//! process on this machine's address on its network, with a certificate of
//! the test's own for the console's hostname.

use magnus_invariants::appliance::{bin, Appliance};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::Stdio;

pub const HOST: &str = "admin.magnus.test";

/// This machine's address on its network: what the console listens on, and
/// where the tests' requests come from.
pub fn lan_ip() -> IpAddr {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
    s.connect("10.255.255.255:1").unwrap();
    let ip = s.local_addr().unwrap().ip();
    assert!(!ip.is_loopback(), "this machine has no address but loopback");
    ip
}

/// A port for a console to listen on, free when asked. Taken below the
/// system's ephemeral range (32768 and up), from a random start: a port the
/// system hands out (port 0, or any outgoing connection of the hundreds of
/// tests running at once) can be taken again before the console binds it,
/// and the test then talks to whatever holds it (another test's console,
/// whose certificate fails as `BadSignature`). Here only another console
/// picking the same port can, one chance in about 12,000.
pub fn free_port(ip: IpAddr) -> u16 {
    const LOW: u16 = 20000;
    const SPAN: u16 = 12000;
    let start = (uuid::Uuid::new_v4().as_u128() % SPAN as u128) as u16;
    (0..SPAN).map(|i| LOW + (start + i) % SPAN).find(|&p| std::net::TcpListener::bind((ip, p)).is_ok()).expect("a free port below the ephemeral range")
}

/// A console made ready to start: where it will listen, its config, and
/// the certificate a browser on the network trusts for it.
pub struct Prepared {
    pub addr: SocketAddr,
    pub config: PathBuf,
    pub ca: reqwest::Certificate,
}

impl Prepared {
    /// Its address as a browser on the network types it.
    pub fn url(&self) -> String {
        format!("https://{HOST}:{}", self.addr.port())
    }
}

pub struct Console {
    pub child: tokio::process::Child,
    pub addr: SocketAddr,
    pub ca: reqwest::Certificate,
}

impl Drop for Console {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

/// `[admin]` with these addresses and subnets.
pub fn admin(listen: &[String], allow: &[&str]) -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("listen".into(), toml::Value::Array(listen.iter().map(|l| l.as_str().into()).collect()));
    t.insert("allow".into(), toml::Value::Array(allow.iter().map(|a| (*a).into()).collect()));
    t
}

/// The appliance's own config with an `[admin]` section, and a certificate
/// for the console's hostname.
pub fn config(app: &Appliance, admin: toml::Table) -> (PathBuf, reqwest::Certificate) {
    let cert = rcgen::generate_simple_self_signed(vec![HOST.to_string()]).unwrap();
    let (c, k) = (app.dir.path().join("admin-cert.pem"), app.dir.path().join("admin-key.pem"));
    std::fs::write(&c, cert.cert.pem()).unwrap();
    std::fs::write(&k, cert.key_pair.serialize_pem()).unwrap();
    let mut cfg: toml::Table = std::fs::read_to_string(app.dir.path().join("magnus.toml")).unwrap().parse().unwrap();
    let mut a = admin;
    a.insert("hostname".into(), HOST.into());
    a.insert("org".into(), "Acme".into());
    a.insert("tls_cert".into(), c.display().to_string().into());
    a.insert("tls_key".into(), k.display().to_string().into());
    cfg.insert("admin".into(), a.into());
    let path = app.dir.path().join(format!("magnus-admin-{}.toml", uuid::Uuid::new_v4()));
    std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
    (path, reqwest::Certificate::from_pem(cert.cert.pem().as_bytes()).unwrap())
}

/// A console on this machine's address taking `allow`, with `more` in its
/// `[admin]` section.
pub fn prepare(app: &Appliance, allow: &[&str], more: &[(&str, toml::Value)]) -> Prepared {
    let ip = lan_ip();
    let addr = SocketAddr::new(ip, free_port(ip));
    let mut a = admin(&[addr.to_string()], allow);
    for (k, v) in more {
        a.insert(k.to_string(), v.clone());
    }
    let (config, ca) = config(app, a);
    Prepared { addr, config, ca }
}

pub async fn launch(p: Prepared) -> Console {
    let child = tokio::process::Command::new(bin("magnus-admin")).arg("--config").arg(&p.config).stdout(Stdio::null()).stderr(Stdio::inherit()).kill_on_drop(true).spawn().unwrap();
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(p.addr).await.is_ok() {
            return Console { child, addr: p.addr, ca: p.ca };
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("magnus-admin did not start on {}", p.addr);
}

pub async fn start(app: &Appliance, allow: &[&str]) -> Console {
    launch(prepare(app, allow, &[])).await
}

/// A browser on the organisation's network: it trusts the console's
/// certificate and finds the hostname at the console's address. It does
/// not follow redirects, so a test sees where the console sends it.
pub fn browser(c: &Console, max_tls12: bool) -> reqwest::Client {
    let mut b = reqwest::Client::builder()
        .timeout(crate::keycloak::HTTP_PATIENCE)
        .add_root_certificate(c.ca.clone())
        .resolve(HOST, c.addr)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none());
    if max_tls12 {
        b = b.max_tls_version(reqwest::tls::Version::TLS_1_2);
    }
    b.build().unwrap()
}

/// A local console user with a password: person-add, role, password.
/// Tests sign in as them through the local flow (#505).
pub fn local_user(app: &Appliance, name: &str, password: &str, admin: bool) {
    let config = app.dir.path().join("magnus.toml");
    // Person ids are lowercase and canonical (#470); the display name keeps
    // its case.
    let id = name.to_lowercase();
    let run = |argv: &[&str]| std::process::Command::new(magnus_invariants::appliance::bin("magnus-appliance")).args(argv).arg("--config").arg(&config).output().unwrap();
    // A fixture person (alice, bob, carol) is already in the config: only
    // a stranger is added.
    let out = run(&["person-add", "--person", &id, "--name", name]);
    assert!(out.status.success() || String::from_utf8_lossy(&out.stderr).contains("already a person"), "magnus person-add: {}", String::from_utf8_lossy(&out.stderr));
    let role = if admin { "admin" } else { "member" };
    let out = run(&["person-role", "--person", &id, "--role", role]);
    assert!(out.status.success(), "magnus person-role: {}", String::from_utf8_lossy(&out.stderr));
    assert!(crate::set_password(app, &id, password), "magnus password for {name}");
}

/// An admin made or unmade at the appliance: `person-status --status`.
pub fn set_status(app: &Appliance, name: &str, status: &str) {
    let out = std::process::Command::new(magnus_invariants::appliance::bin("magnus-appliance"))
        .args(["person-status", "--person", name, "--status", status, "--config"])
        .arg(app.dir.path().join("magnus.toml"))
        .output()
        .unwrap();
    assert!(out.status.success(), "magnus person-status {name} {status}: {}", String::from_utf8_lossy(&out.stderr));
}

/// The browser signs in as `who` (and steps up after, if asked): the
/// console's session cookie, or the script's log when it fails.
pub async fn sign_in(p_url: &str, at: std::net::SocketAddr, who: &str, password: &str, expect: &str, step_up: bool) -> Result<String, String> {
    sign_in_and_visit(p_url, at, who, password, expect, step_up, None).await
}

/// The same, then the screen `visit` (e.g. `#/users`) checked in every look
/// and at phone width.
pub async fn sign_in_and_visit(p_url: &str, at: std::net::SocketAddr, who: &str, password: &str, expect: &str, step_up: bool, visit: Option<&str>) -> Result<String, String> {
    sign_in_with(p_url, at, who, password, expect, step_up, visit, &[]).await
}

/// The same, with more of the script's settings (`MAGNUS_SHELL` and its
/// own, in `desktop/tests/admin-sign-in.mjs`).
pub async fn sign_in_with(
    p_url: &str,
    at: std::net::SocketAddr,
    who: &str,
    password: &str,
    expect: &str,
    step_up: bool,
    visit: Option<&str>,
    more: &[(&str, String)],
) -> Result<String, String> {
    let out = tokio::process::Command::new("node")
        .arg(magnus_invariants::appliance::workspace_root().join("desktop/tests/admin-sign-in.mjs"))
        .env("MAGNUS_ADMIN_URL", p_url)
        .env("MAGNUS_ADMIN_AT", at.to_string())
        .env("MAGNUS_PERSON", who.to_lowercase())
        .env("MAGNUS_PASSWORD", password)
        .env("MAGNUS_EXPECT", expect)
        .env("MAGNUS_STEP_UP", if step_up { "1" } else { "0" })
        .envs(visit.map(|v| ("MAGNUS_VISIT", v)))
        .envs(more.iter().map(|(k, v)| (*k, v.as_str())))
        .env("MAGNUS_CHROMIUM", crate::chromium())
        .envs(crate::shots())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(std::time::Duration::from_secs(240), out).await.map_err(|_| "the browser did not finish in 240 seconds".to_string())?.unwrap();
    let log = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if !out.status.success() {
        return Err(log);
    }
    Ok(String::from_utf8_lossy(&out.stdout).lines().last().unwrap_or_default().trim().to_string())
}

pub fn with_cookie(b: reqwest::RequestBuilder, sid: &str) -> reqwest::RequestBuilder {
    b.header("cookie", format!("__Host-magnus_admin={sid}"))
}

/// A stand-in for `tailscale cert` (a test fake): it hands the console the
/// certificate the test has made ready, issued for the name asked by a CA
/// of the test's own that stands for Let's Encrypt, or fails saying what
/// tailscale says when the tailnet has HTTPS turned off.
pub struct FakeTailscale {
    dir: tempfile::TempDir,
    ca: rcgen::Certificate,
    ca_key: rcgen::KeyPair,
    pub program: PathBuf,
}

impl FakeTailscale {
    pub fn new() -> FakeTailscale {
        let dir = tempfile::tempdir().unwrap();
        let mut p = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        p.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        p.distinguished_name.push(rcgen::DnType::CommonName, "A public CA browsers trust (test)");
        let ca_key = rcgen::KeyPair::generate().unwrap();
        let ca = p.self_signed(&ca_key).unwrap();
        let program = dir.path().join("tailscale");
        let d = dir.path().display();
        // tailscale cert --cert-file C --key-file K NAME
        std::fs::write(
            &program,
            format!("#!/bin/sh\n[ \"$1\" = cert ] || exit 2\necho \"$6\" >> '{d}/asked'\n[ -f '{d}/fail' ] && {{ cat '{d}/fail' >&2; exit 1; }}\n[ \"$6\" = \"$(cat '{d}/name')\" ] || {{ echo \"invalid domain $6\" >&2; exit 1; }}\ncp '{d}/next.pem' \"$3\" && cp '{d}/next.key' \"$5\"\n"),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        FakeTailscale { dir, ca, ca_key, program }
    }

    /// The next certificate it hands out, for `name`: its DER.
    pub fn ready(&self, name: &str) -> Vec<u8> {
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = rcgen::CertificateParams::new(vec![name.to_string()]).unwrap().signed_by(&key, &self.ca, &self.ca_key).unwrap();
        std::fs::write(self.dir.path().join("name"), name).unwrap();
        std::fs::write(self.dir.path().join("next.pem"), cert.pem()).unwrap();
        std::fs::write(self.dir.path().join("next.key"), key.serialize_pem()).unwrap();
        let _ = std::fs::remove_file(self.dir.path().join("fail"));
        cert.der().to_vec()
    }

    /// From now on it fails, saying `why`.
    pub fn fail(&self, why: &str) {
        std::fs::write(self.dir.path().join("fail"), why).unwrap();
    }

    /// How many times it was asked.
    pub fn asked(&self) -> usize {
        std::fs::read_to_string(self.dir.path().join("asked")).map(|s| s.lines().count()).unwrap_or(0)
    }

    /// The CA browsers trust, for a client.
    pub fn public_ca(&self) -> reqwest::Certificate {
        reqwest::Certificate::from_pem(self.ca.pem().as_bytes()).unwrap()
    }

    /// `[admin]` keys that have the console use it, keeping the issued
    /// certificate in `dir`.
    pub fn config(&self, dir: &std::path::Path) -> Vec<(&'static str, toml::Value)> {
        vec![
            ("trusted_via", "tailscale".into()),
            ("trusted_cert", dir.join("trusted.pem").display().to_string().into()),
            ("trusted_key", dir.join("trusted.key").display().to_string().into()),
            ("tailscale", self.program.display().to_string().into()),
        ]
    }
}

impl Default for FakeTailscale {
    fn default() -> Self {
        Self::new()
    }
}

/// The certificate the console presents to a browser naming its hostname,
/// as DER, through a fresh connection that trusts `ca` alone; or why the
/// browser refused it.
pub async fn presented(c: &Console, ca: reqwest::Certificate) -> Result<Vec<u8>, String> {
    presented_at(c.addr, HOST, ca).await
}

/// The same, for a console at `addr` named `host`.
pub async fn presented_at(addr: SocketAddr, host: &str, ca: reqwest::Certificate) -> Result<Vec<u8>, String> {
    let b = reqwest::Client::builder()
        .timeout(crate::keycloak::HTTP_PATIENCE)
        .add_root_certificate(ca)
        .tls_built_in_root_certs(false)
        .tls_info(true)
        .resolve(host, addr)
        .no_proxy()
        .build()
        .unwrap();
    let r = b.get(format!("https://{host}:{}/status", addr.port())).send().await.map_err(|e| format!("{e:?}"))?;
    r.extensions().get::<reqwest::tls::TlsInfo>().and_then(|i| i.peer_certificate()).map(<[u8]>::to_vec).ok_or_else(|| "no certificate seen".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Binding port 0 hands back a port in the kernel's ephemeral range.
    /// A console's port has to be chosen in 20000-31999, below that range,
    /// or another test can take it before the console binds it.
    #[test]
    fn a_console_port_is_below_the_ephemeral_range() {
        let ip = IpAddr::from([127, 0, 0, 1]);
        for _ in 0..4 {
            let port = free_port(ip);
            assert!((20000..=31999).contains(&port), "port {port} is not in 20000-31999");
        }
    }
}
