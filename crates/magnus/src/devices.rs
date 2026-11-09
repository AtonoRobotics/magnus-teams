//! A computer paired with the appliance, and the client certificate
//! Cloudflare's managed CA issued for it.
//!
//! The private key is made on the computer and never sent. The appliance
//! receives a certificate request, asks Cloudflare to sign it, and keeps
//! only the certificate's fingerprint. Cloudflare refuses a connection to
//! the public name that does not present one of these certificates; the
//! check here is the same rule at the origin, for a request the tunnel
//! forwarded. A certificate the appliance signed itself would not satisfy
//! Cloudflare, and bringing our own CA is an Enterprise feature, so the
//! zone's managed CA is the one that can enforce this on the plan the zone
//! is on.

use crate::vault::write_private;
use anyhow::{Context, Result};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Cloudflare's API. Tests pass their own server; the config does not.
pub const CF_API: &str = "https://api.cloudflare.com/client/v4";

/// The header a Transform Rule sets from `cf.tls_client_auth.cert_fingerprint_sha256`.
pub const CERT_HEADER: &str = "x-magnus-client-cert";

const MAX_PER_PERSON: usize = 20;
const MAX_PER_HOUR: usize = 8;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Device {
    pub id: String,
    pub person: String,
    pub name: String,
    /// Cloudflare's id for the certificate, which is what a revocation deletes.
    pub cloudflare_id: String,
    /// SHA-256 of the certificate, hex with no colons.
    pub fingerprint: String,
    /// The fingerprint Cloudflare returned, normalized the same way. The edge
    /// and the API have been seen to punctuate it differently.
    #[serde(default)]
    pub cloudflare_fingerprint: String,
    #[serde(default)]
    pub serial: String,
    pub created: DateTime<Utc>,
    #[serde(default)]
    pub revoked: Option<DateTime<Utc>>,
}

impl Device {
    fn matches(&self, header: &str) -> bool {
        let n = normalize_fp(header);
        !n.is_empty() && (n == self.fingerprint || n == self.cloudflare_fingerprint)
    }

    pub fn live(&self) -> bool {
        self.revoked.is_none()
    }
}

/// What pairing hands back. There is no private key: the computer kept it.
#[derive(Clone, Debug)]
pub struct Issued {
    pub device: String,
    pub fingerprint: String,
    pub certificate: String,
}

#[derive(Debug)]
pub enum IssueError {
    Rejected(&'static str),
    Limited(&'static str),
    Unavailable(String),
    Upstream(String),
}

impl std::fmt::Display for IssueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IssueError::Rejected(m) | IssueError::Limited(m) => f.write_str(m),
            IssueError::Unavailable(m) | IssueError::Upstream(m) => f.write_str(m),
        }
    }
}

/// The Cloudflare call. `token` is the vault secret; it is not logged.
pub struct Cloudflare {
    pub api: String,
    pub zone: String,
    pub token: String,
    pub days: u32,
}

struct Registry {
    devices: BTreeMap<String, Device>,
    /// When each person paired, so a burst stops before the zone's quota does.
    issued: BTreeMap<String, Vec<DateTime<Utc>>>,
}

pub struct Devices {
    path: PathBuf,
    inner: Mutex<Registry>,
}

fn normalize_fp(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_hexdigit()).map(|c| c.to_ascii_lowercase()).collect()
}

fn pem_der(pem: &str) -> Result<Vec<u8>> {
    let b64: String = pem.lines().filter(|l| !l.starts_with('-') && !l.trim().is_empty()).collect();
    base64::engine::general_purpose::STANDARD.decode(b64.trim()).context("the certificate is not PEM")
}

fn fingerprint_of(pem: &str) -> Result<String> {
    Ok(hex::encode(sha2::Sha256::digest(pem_der(pem)?)))
}

/// A request the tunnel forwarded: cloudflared connects from loopback and
/// sets `Cf-Connecting-Ip`. The same headers from any other address are the
/// client's own and are ignored.
pub fn is_tunnel(peer: SocketAddr, headers: &axum::http::HeaderMap) -> bool {
    peer.ip().to_canonical().is_loopback() && headers.contains_key("cf-connecting-ip")
}

pub fn presented<'a>(headers: &'a axum::http::HeaderMap) -> Option<&'a str> {
    headers.get(CERT_HEADER).and_then(|v| v.to_str().ok()).map(str::trim).filter(|s| !s.is_empty())
}

/// Whether this request may proceed once device certificates are configured.
/// `live` is whether the presented fingerprint is a paired device that has
/// not been revoked. Health stays open so a probe can see the process; the
/// edge still requires a certificate before a probe from the internet arrives.
pub fn admit(on: bool, peer: SocketAddr, headers: &axum::http::HeaderMap, method: &axum::http::Method, path: &str, live: bool) -> Result<(), &'static str> {
    if !on || !is_tunnel(peer, headers) {
        return Ok(());
    }
    if method == axum::http::Method::GET && (path == "/v1/health" || path == "/oauth/slack/callback") {
        return Ok(());
    }
    if live {
        Ok(())
    } else {
        Err("this device is not paired with the appliance")
    }
}

impl Devices {
    pub fn open(state_dir: &Path) -> Result<Devices> {
        let dir = state_dir.join("accounts");
        std::fs::create_dir_all(&dir)?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        let path = dir.join("devices.json");
        let devices = match std::fs::read(&path) {
            Ok(b) => serde_json::from_slice(&b).with_context(|| format!("{} does not parse", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Devices { path, inner: Mutex::new(Registry { devices, issued: BTreeMap::new() }) })
    }

    fn save(&self, devices: &BTreeMap<String, Device>) -> Result<()> {
        write_private(&self.path, &serde_json::to_vec_pretty(devices)?)
    }

    pub fn get(&self, id: &str) -> Option<Device> {
        self.inner.lock().unwrap().devices.get(id).cloned()
    }

    pub fn list(&self, person: Option<&str>) -> Vec<Device> {
        let g = self.inner.lock().unwrap();
        let mut out: Vec<Device> = g.devices.values().filter(|d| person.map(|p| d.person == p).unwrap_or(true)).cloned().collect();
        out.sort_by(|a, b| b.created.cmp(&a.created));
        out
    }

    pub fn is_live(&self, fingerprint: &str) -> bool {
        self.live_id(fingerprint).is_some()
    }

    pub fn live_id(&self, fingerprint: &str) -> Option<String> {
        let g = self.inner.lock().unwrap();
        g.devices.values().find(|d| d.live() && d.matches(fingerprint)).map(|d| d.id.clone())
    }

    /// Have Cloudflare sign `csr` and remember the device. The private key
    /// is not a parameter and is not stored.
    pub async fn issue(&self, cf: &Cloudflare, person: &str, name: &str, csr: &str) -> std::result::Result<Issued, IssueError> {
        let name = clean_name(name)?;
        let csr = clean_csr(csr)?;
        if !(1..=3650).contains(&cf.days) {
            return Err(IssueError::Unavailable("device certificates are set to a lifetime Cloudflare will not issue (1 to 3650 days)".into()));
        }
        if cf.zone.trim().is_empty() || cf.token.trim().is_empty() {
            return Err(IssueError::Unavailable("device certificates need a zone id and a Cloudflare token".into()));
        }
        {
            let g = self.inner.lock().unwrap();
            let live = g.devices.values().filter(|d| d.person == person && d.live()).count();
            if live >= MAX_PER_PERSON {
                return Err(IssueError::Limited("This account already has 20 paired devices. Revoke one before pairing another."));
            }
            let recent = g.issued.get(person).map(|v| v.iter().filter(|t| Utc::now() - **t < Duration::hours(1)).count()).unwrap_or(0);
            if recent >= MAX_PER_HOUR {
                return Err(IssueError::Limited("Too many devices were paired from this account. Try again in an hour."));
            }
        }
        let signed = cloudflare_sign(cf, &csr).await?;
        let fingerprint = fingerprint_of(&signed.certificate).map_err(|e| IssueError::Upstream(format!("{e:#}")))?;
        let now = Utc::now();
        let id = crate::kernel::new_id("dev");
        let device = Device {
            id: id.clone(),
            person: person.to_string(),
            name,
            cloudflare_id: signed.id,
            fingerprint: fingerprint.clone(),
            cloudflare_fingerprint: normalize_fp(&signed.fingerprint),
            serial: signed.serial,
            created: now,
            revoked: None,
        };
        let cf_id = device.cloudflare_id.clone();
        let saved = {
            let mut g = self.inner.lock().unwrap();
            g.devices.insert(id.clone(), device);
            g.issued.entry(person.to_string()).or_default().push(now);
            match self.save(&g.devices) {
                Ok(()) => Ok(()),
                Err(e) => {
                    g.devices.remove(&id);
                    Err(e)
                }
            }
        };
        if let Err(e) = saved {
            let _ = cloudflare_revoke(cf, &cf_id).await;
            return Err(IssueError::Unavailable(format!("{e:#}")));
        }
        Ok(Issued { device: id, fingerprint, certificate: signed.certificate })
    }

    /// Tell Cloudflare to drop the certificate, then forget it here. A 404
    /// means it is already gone, which is the state we want.
    pub async fn revoke(&self, cf: &Cloudflare, id: &str) -> std::result::Result<Device, IssueError> {
        let cloudflare_id = {
            let g = self.inner.lock().unwrap();
            match g.devices.get(id) {
                Some(d) if d.live() => d.cloudflare_id.clone(),
                Some(_) => return Err(IssueError::Rejected("that device is already revoked")),
                None => return Err(IssueError::Rejected("no such device")),
            }
        };
        cloudflare_revoke(cf, &cloudflare_id).await?;
        let mut g = self.inner.lock().unwrap();
        let Some(d) = g.devices.get_mut(id) else { return Err(IssueError::Rejected("no such device")) };
        d.revoked = Some(Utc::now());
        let out = d.clone();
        self.save(&g.devices).map_err(|e| IssueError::Unavailable(format!("{e:#}")))?;
        Ok(out)
    }
}

fn clean_name(name: &str) -> std::result::Result<String, IssueError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(|c| c.is_control()) {
        return Err(IssueError::Rejected("Name the device in 80 characters or fewer, with no control characters."));
    }
    Ok(name.to_string())
}

fn clean_csr(csr: &str) -> std::result::Result<String, IssueError> {
    let csr = csr.trim();
    if csr.len() < 64 || csr.len() > 16 * 1024 || !csr.contains("BEGIN CERTIFICATE REQUEST") || csr.contains('\0') {
        return Err(IssueError::Rejected("Send the device's certificate request."));
    }
    Ok(csr.to_string())
}

struct CfCert {
    id: String,
    certificate: String,
    fingerprint: String,
    serial: String,
}

async fn cloudflare_sign(cf: &Cloudflare, csr: &str) -> std::result::Result<CfCert, IssueError> {
    let url = format!("{}/zones/{}/client_certificates", cf.api.trim_end_matches('/'), cf.zone.trim());
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build().map_err(|e| IssueError::Upstream(e.to_string()))?;
    let r = client
        .post(&url)
        .bearer_auth(&cf.token)
        .json(&serde_json::json!({ "csr": csr, "validity_days": cf.days }))
        .send()
        .await
        .map_err(|e| IssueError::Upstream(e.to_string()))?;
    let status = r.status();
    let body: serde_json::Value = r.json().await.map_err(|e| IssueError::Upstream(e.to_string()))?;
    if !status.is_success() || body.get("success") == Some(&serde_json::Value::Bool(false)) {
        let msg = body["errors"][0]["message"].as_str().unwrap_or("Cloudflare refused the certificate");
        return Err(IssueError::Upstream(msg.to_string()));
    }
    let result = body.get("result").cloned().unwrap_or(body);
    let certificate = result["certificate"].as_str().unwrap_or("").to_string();
    let id = result["id"].as_str().unwrap_or("").to_string();
    if certificate.is_empty() || id.is_empty() {
        return Err(IssueError::Upstream("Cloudflare answered without a certificate".into()));
    }
    Ok(CfCert { id, certificate, fingerprint: result["fingerprint_sha256"].as_str().unwrap_or("").to_string(), serial: result["serial_number"].as_str().unwrap_or("").to_string() })
}

async fn cloudflare_revoke(cf: &Cloudflare, id: &str) -> std::result::Result<(), IssueError> {
    let url = format!("{}/zones/{}/client_certificates/{}", cf.api.trim_end_matches('/'), cf.zone.trim(), id);
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build().map_err(|e| IssueError::Upstream(e.to_string()))?;
    let r = client.delete(&url).bearer_auth(&cf.token).send().await.map_err(|e| IssueError::Upstream(e.to_string()))?;
    if r.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(());
    }
    let status = r.status();
    let body: serde_json::Value = r.json().await.unwrap_or(serde_json::Value::Null);
    if !status.is_success() || body.get("success") == Some(&serde_json::Value::Bool(false)) {
        let msg = body["errors"][0]["message"].as_str().unwrap_or("Cloudflare did not revoke the certificate");
        return Err(IssueError::Upstream(msg.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::Path, routing::post, Json, Router};
    use std::sync::Arc;

    fn csr() -> String {
        format!("-----BEGIN CERTIFICATE REQUEST-----\n{}\n-----END CERTIFICATE REQUEST-----", "A".repeat(80))
    }

    fn cert_pem() -> String {
        "-----BEGIN CERTIFICATE-----\nYQ==\n-----END CERTIFICATE-----\n".into()
    }

    async fn cloudflare() -> (String, Arc<Mutex<Vec<String>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let posted = calls.clone();
        let deleted = calls.clone();
        let app = Router::new()
            .route(
                "/zones/{zone}/client_certificates",
                post(move |Json(b): Json<serde_json::Value>| {
                    let posted = posted.clone();
                    async move {
                        posted.lock().unwrap().push(format!("POST {}", b["validity_days"]));
                        Json(serde_json::json!({
                            "success": true,
                            "result": {
                                "id": "cf-cert-1",
                                "certificate": cert_pem(),
                                "fingerprint_sha256": "61:61",
                                "serial_number": "10"
                            }
                        }))
                    }
                }),
            )
            .route(
                "/zones/{zone}/client_certificates/{id}",
                axum::routing::delete(move |Path((_, id)): Path<(String, String)>| {
                    let deleted = deleted.clone();
                    async move {
                        deleted.lock().unwrap().push(format!("DELETE {id}"));
                        Json(serde_json::json!({ "success": true, "result": { "id": id } }))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}"), calls)
    }

    fn cf(api: String) -> Cloudflare {
        Cloudflare { api, zone: "zone1".into(), token: "token".into(), days: 365 }
    }

    #[test]
    fn the_slack_callback_needs_no_device_certificate() {
        let peer: std::net::SocketAddr = "127.0.0.1:9".parse().unwrap();
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("cf-connecting-ip", "203.0.113.8".parse().unwrap());
        let get = axum::http::Method::GET;
        assert!(admit(true, peer, &headers, &get, "/oauth/slack/callback", false).is_ok());
        assert!(admit(true, peer, &headers, &axum::http::Method::POST, "/oauth/slack/callback", false).is_err());
        assert!(admit(true, peer, &headers, &get, "/v1/connectors", false).is_err());
        assert!(admit(true, peer, &headers, &get, "/v1/health", false).is_ok());
    }

    #[tokio::test]
    async fn pairing_keeps_no_private_key_and_revocation_reaches_cloudflare() {
        let dir = tempfile::tempdir().unwrap();
        let devices = Devices::open(dir.path()).unwrap();
        let (api, calls) = cloudflare().await;
        let issued = devices.issue(&cf(api), "ada", "Ada's laptop", &csr()).await.unwrap();
        assert!(!issued.certificate.contains("PRIVATE KEY"));
        let raw = std::fs::read_to_string(dir.path().join("accounts/devices.json")).unwrap();
        assert!(!raw.contains("PRIVATE KEY"), "the registry held a private key");
        assert!(!raw.contains("BEGIN CERTIFICATE REQUEST"), "the registry held the request");
        assert!(raw.contains(&issued.fingerprint));
        assert!(devices.is_live("61:61"), "Cloudflare's punctuation of the fingerprint did not match");
        assert!(devices.is_live(&issued.fingerprint));
        assert_eq!(calls.lock().unwrap().as_slice(), ["POST 365"]);
        let listed = devices.list(Some("ada"));
        assert_eq!(listed.len(), 1);
        assert!(devices.list(Some("other")).is_empty());
    }

    #[tokio::test]
    async fn a_revoked_device_is_dead_here_and_deleted_at_cloudflare() {
        let dir = tempfile::tempdir().unwrap();
        let devices = Devices::open(dir.path()).unwrap();
        let (api, calls) = cloudflare().await;
        let issued = devices.issue(&cf(api.clone()), "ada", "laptop", &csr()).await.unwrap();
        devices.revoke(&cf(api), &issued.device).await.unwrap();
        assert!(!devices.is_live(&issued.fingerprint));
        assert_eq!(calls.lock().unwrap().last().map(String::as_str), Some("DELETE cf-cert-1"));
        let again = devices.revoke(&cf("http://127.0.0.1:1".into()), &issued.device).await;
        assert!(matches!(again, Err(IssueError::Rejected(_))));
    }

    #[tokio::test]
    async fn the_ninth_pairing_in_an_hour_is_refused_before_cloudflare() {
        let dir = tempfile::tempdir().unwrap();
        let devices = Devices::open(dir.path()).unwrap();
        let (api, calls) = cloudflare().await;
        for i in 0..MAX_PER_HOUR {
            // Each issue needs a distinct Cloudflare id or the map just replaces.
            // The mock returns one id; the device id is ours, so the map grows.
            devices.issue(&cf(api.clone()), "ada", &format!("box {i}"), &csr()).await.unwrap();
        }
        let extra = devices.issue(&cf(api), "ada", "one more", &csr()).await;
        assert!(matches!(extra, Err(IssueError::Limited(_))));
        assert_eq!(calls.lock().unwrap().len(), MAX_PER_HOUR);
    }

    #[test]
    fn twenty_live_devices_stop_another() {
        let dir = tempfile::tempdir().unwrap();
        let devices = Devices::open(dir.path()).unwrap();
        {
            let mut g = devices.inner.lock().unwrap();
            for i in 0..MAX_PER_PERSON {
                let id = format!("dev_{i}");
                g.devices.insert(
                    id.clone(),
                    Device {
                        id,
                        person: "ada".into(),
                        name: "n".into(),
                        cloudflare_id: format!("c{i}"),
                        fingerprint: format!("{i:064x}"),
                        cloudflare_fingerprint: String::new(),
                        serial: String::new(),
                        created: Utc::now(),
                        revoked: None,
                    },
                );
            }
        }
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let err = rt.block_on(devices.issue(&cf("http://127.0.0.1:1".into()), "ada", "another", &csr()));
        assert!(matches!(err, Err(IssueError::Limited(_))));
    }

    #[test]
    fn the_tunnel_requires_a_live_device_and_the_lan_does_not() {
        let lan: SocketAddr = "192.168.0.10:40000".parse().unwrap();
        let local: SocketAddr = "127.0.0.1:40000".parse().unwrap();
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("cf-connecting-ip", "203.0.113.5".parse().unwrap());
        assert!(admit(true, lan, &headers, &axum::http::Method::GET, "/v1/me", false).is_ok(), "a LAN client was held to a device certificate");
        assert!(admit(true, local, &axum::http::HeaderMap::new(), &axum::http::Method::POST, "/v1/sign-in", false).is_ok(), "a local request was treated as the tunnel");
        assert!(admit(false, local, &headers, &axum::http::Method::GET, "/v1/me", false).is_ok());
        assert!(admit(true, local, &headers, &axum::http::Method::GET, "/v1/health", false).is_ok());
        assert_eq!(admit(true, local, &headers, &axum::http::Method::POST, "/v1/sign-in", false).unwrap_err(), "this device is not paired with the appliance");
        assert!(admit(true, local, &headers, &axum::http::Method::GET, "/v1/me", true).is_ok());
        // A fingerprint header from the LAN does not make it a tunnel, and
        // does not have to be live.
        headers.insert(CERT_HEADER, "deadbeef".parse().unwrap());
        assert!(admit(true, lan, &headers, &axum::http::Method::POST, "/v1/tasks", false).is_ok());
    }

    #[tokio::test]
    async fn a_bad_request_never_reaches_cloudflare() {
        let dir = tempfile::tempdir().unwrap();
        let devices = Devices::open(dir.path()).unwrap();
        let err = devices.issue(&cf("http://127.0.0.1:1".into()), "ada", "ok", "not a request").await;
        assert!(matches!(err, Err(IssueError::Rejected(_))));
    }
}
