//! The way out of a computer: a forward proxy Magnus runs, reached only
//! through each computer's forwarder. The socket names the computer and
//! nothing the guest sends can.

mod audit;
mod ca;
mod http;
mod policy;
pub(super) mod surrogate;
pub(super) mod watch;

pub use audit::migrate_sql;
pub(super) use ca::certificate_pem;
pub(super) use ca::{secret_name as ca_secret_name, SECRET_PREFIX as CA_SECRET_PREFIX};

use super::client;
use crate::kernel::Kernel;
use anyhow::{Context, Result};
use rustls::pki_types::pem::PemObject;
use serde_json::json;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::net::{UnixListener, UnixStream};

const BIND_RETRY: Duration = Duration::from_secs(5);
const SOCKET_MODE: u32 = 0o660;
const MAX_CONNECTIONS: usize = 1024;

/// Downloads kept per computer, to check a manifest entry against.
const MAX_DOWNLOADS_REMEMBERED: usize = 512;

type Downloads = HashMap<String, VecDeque<(String, String)>>;

fn downloads() -> &'static Mutex<Downloads> {
    static SEEN: OnceLock<Mutex<Downloads>> = OnceLock::new();
    SEEN.get_or_init(Default::default)
}

fn saw_download(computer: &str, host: &str, sha256: String) {
    let mut seen = downloads().lock().unwrap_or_else(|e| e.into_inner());
    let mine = seen.entry(computer.to_string()).or_default();
    if mine.len() == MAX_DOWNLOADS_REMEMBERED {
        mine.pop_front();
    }
    mine.push_back((host.to_ascii_lowercase(), sha256));
}

/// Whether this proxy carried this file from this host to this computer.
pub(in crate::computer) fn carried(computer: &str, host: &str, sha256: &str) -> bool {
    let seen = downloads().lock().unwrap_or_else(|e| e.into_inner());
    let host = host.to_ascii_lowercase();
    seen.get(computer).is_some_and(|mine| mine.iter().any(|(h, s)| *h == host && s == sha256))
}

struct Proxy {
    kernel: Arc<Kernel>,
    audit: audit::Audit,
    learned: Mutex<policy::Learned>,
    /// Answers already given for a scripted name, to walk its list.
    asked: Mutex<HashMap<String, usize>>,
    client_tls: Arc<rustls::ClientConfig>,
}

/// Starts when computerd answers, so either can come up first.
pub async fn serve(kernel: Arc<Kernel>, computerd: PathBuf) {
    let path = loop {
        match client::ask(&computerd, json!({ "op": "egress_socket" })).await {
            Ok(v) => break PathBuf::from(v["socket"].as_str().unwrap_or_default()),
            Err(_) => tokio::time::sleep(BIND_RETRY).await,
        }
    };
    if let Err(e) = listen(kernel, &computerd, &path).await {
        tracing::error!("the computers' egress proxy stopped: {e:#}");
    }
}

async fn listen(kernel: Arc<Kernel>, computerd: &Path, path: &Path) -> Result<()> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path).with_context(|| format!("listen on {}", path.display()))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(SOCKET_MODE)).context("set the proxy socket's mode")?;
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(file) = kernel.cfg.computer.as_ref().and_then(|c| c.upstream_ca.as_ref()) {
        for certificate in rustls::pki_types::CertificateDer::pem_file_iter(file).with_context(|| format!("read {}", file.display()))? {
            roots.add(certificate.context("a certificate that cannot be read")?).context("a certificate that cannot be trusted")?;
        }
    }
    let client_tls = rustls::ClientConfig::builder_with_provider(rustls::crypto::ring::default_provider().into())
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let proxy = Arc::new(Proxy {
        audit: audit::Audit::new(&kernel.cfg.database.url),
        kernel,
        learned: Mutex::new(HashMap::new()),
        asked: Mutex::new(HashMap::new()),
        client_tls: Arc::new(client_tls),
    });
    let limit = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (conn, _) = listener.accept().await?;
        let Ok(permit) = limit.clone().try_acquire_owned() else { continue };
        let (proxy, computerd) = (proxy.clone(), computerd.to_path_buf());
        tokio::spawn(async move {
            let _permit = permit;
            if let Err(e) = connection(proxy, &computerd, conn).await {
                tracing::debug!("egress connection ended: {e:#}");
            }
        });
    }
}

/// The kernel says who connected; computerd says which computer that uid is.
async fn connection(proxy: Arc<Proxy>, computerd: &Path, conn: UnixStream) -> Result<()> {
    let uid = conn.peer_cred().context("who connected")?.uid();
    let reply = client::ask(computerd, json!({ "op": "whose", "uid": uid })).await?;
    let id = reply["id"].as_str().context("a connection from no computer")?.to_string();
    http::serve_connection(proxy, id, conn).await
}

/// Lookup answers set in the config, for a name with no real DNS behind it.
#[derive(Clone, Default)]
struct Scripted<'a> {
    public: Option<&'a BTreeMap<String, Vec<String>>>,
    checked: Option<&'a BTreeMap<String, Vec<String>>>,
}

fn private(ip: IpAddr) -> bool {
    crate::web::private_ip(ip)
}

impl Proxy {
    /// One lookup, whose answer is the only address connected to, so the name
    /// cannot change between this check and the connection.
    async fn resolve(&self, host: &str, port: u16) -> Result<SocketAddr, String> {
        let Some(settings) = self.kernel.cfg.computer.as_ref() else { return Err("no computers here".into()) };
        let script = Scripted { public: Some(&settings.public_hosts), checked: Some(&settings.hosts) };
        let answer = |list: &[String]| -> Option<String> {
            let mut asked = self.asked.lock().unwrap_or_else(|e| e.into_inner());
            let at = asked.entry(host.to_string()).or_insert(0);
            let got = list.get((*at).min(list.len().saturating_sub(1))).cloned();
            *at += 1;
            got
        };
        if let Some(addr) = script.public.and_then(|m| m.get(host)).and_then(|l| answer(l)) {
            return addr.parse().map_err(|_| format!("a bad configured address for {host}"));
        }
        let addrs: Vec<SocketAddr> = if let Some(addr) = script.checked.and_then(|m| m.get(host)).and_then(|l| answer(l)) {
            vec![addr.parse().map_err(|_| format!("a bad configured address for {host}"))?]
        } else if let Ok(ip) = host.parse::<IpAddr>() {
            vec![SocketAddr::new(ip, port)]
        } else {
            tokio::net::lookup_host((host, port)).await.map_err(|e| format!("{host} does not resolve: {e}"))?.collect()
        };
        if addrs.is_empty() {
            return Err(format!("{host} does not resolve"));
        }
        // Every answer, or a mix of one public and one private would pass.
        if let Some(bad) = addrs.iter().find(|a| private(a.ip())) {
            return Err(format!("{host} resolves to a private address, {}", bad.ip()));
        }
        Ok(addrs[0])
    }
}
