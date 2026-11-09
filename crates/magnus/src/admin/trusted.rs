//! The console's certificates, kept current while it runs (admin console
//! spec, Access: "a certificate browsers already trust: ACME DNS-01 under a
//! domain the organisation controls, or the organisation's own CA").
//!
//! With `admin.trusted_via = "tailscale"`, the machine's tailnet has a
//! certificate issued for the console's hostname (its name on the tailnet,
//! under the tailnet's own domain): Let's Encrypt, by ACME DNS-01, which
//! tailscaled answers for. The console asks for it on start when it is not
//! yet issued, every twelve hours when fewer than 30 days are left, and on
//! Operations › Network › Renew; each time it reads the organisation's own
//! certificate (`tls_cert`) again too. Browsers naming the hostname get the
//! trusted certificate once it is issued; every other name (the LAN
//! address, the tailnet address), and every name until then, gets the
//! organisation's own. A failure keeps what is served and says why
//! (Overview's alerts, Operations › Network); it never stops the console.

use super::net::{self, Certs};
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Renewed when fewer days than this are left (Let's Encrypt's last 30 of 90).
const RENEW_DAYS: i64 = 30;
/// How often the console looks.
const EVERY: Duration = Duration::from_secs(12 * 3600);

struct Issued {
    cert: PathBuf,
    key: PathBuf,
    tailscale: PathBuf,
}

pub struct Tls {
    pub certs: Arc<Certs>,
    hostname: String,
    own: (PathBuf, PathBuf),
    trusted: Option<Issued>,
    /// Why the trusted certificate could not be issued, the last time it
    /// was asked for; None once it was.
    pub error: std::sync::RwLock<Option<String>>,
    /// One renewal at a time.
    busy: tokio::sync::Mutex<()>,
}

impl Tls {
    /// The organisation's certificate, which must be readable for the
    /// console to start, and a trusted one already issued, if any.
    pub fn new(a: &crate::config::Admin) -> Result<Tls> {
        let certs = Certs::new(&a.hostname, net::certified(&a.tls_cert, &a.tls_key)?);
        let trusted = match a.trusted_via.as_deref() {
            None => None,
            Some("tailscale") => Some(Issued {
                cert: a.trusted_cert.clone().context("admin.trusted_via = \"tailscale\" needs trusted_cert: where the issued certificate is kept")?,
                key: a.trusted_key.clone().context("admin.trusted_via = \"tailscale\" needs trusted_key: where its key is kept")?,
                tailscale: a.tailscale.clone().unwrap_or_else(|| "tailscale".into()),
            }),
            Some(other) => bail!(
                "admin.trusted_via {other:?}: only \"tailscale\" has a certificate issued here; for the organisation's own CA, leave it out and put its certificate in tls_cert"
            ),
        };
        if let Some(t) = &trusted {
            match net::certified(&t.cert, &t.key) {
                Ok(k) => certs.set_trusted(k),
                Err(e) => tracing::info!("admin console: no trusted certificate issued yet ({e:#}); asking for one"),
            }
        }
        Ok(Tls { certs, hostname: a.hostname.to_ascii_lowercase(), own: (a.tls_cert.clone(), a.tls_key.clone()), trusted, error: Default::default(), busy: Default::default() })
    }

    /// How the trusted certificate is had (`"tailscale"`), if one is.
    pub fn via(&self) -> Option<&'static str> {
        self.trusted.as_ref().map(|_| "tailscale")
    }

    /// The trusted certificate's file, for its expiry.
    pub fn trusted_path(&self) -> Option<PathBuf> {
        self.trusted.as_ref().map(|t| t.cert.clone())
    }

    /// Read the organisation's certificate again and have the trusted one
    /// issued or renewed: what an admin's Renew does.
    pub async fn renew(&self) -> Result<()> {
        let _one = self.busy.lock().await;
        self.certs.set_own(net::certified(&self.own.0, &self.own.1).context("the organisation's certificate")?);
        let Some(t) = &self.trusted else { return Ok(()) };
        let r = issue(t, &self.hostname).await;
        match &r {
            Ok(k) => {
                self.certs.set_trusted(k.clone());
                *self.error.write().unwrap() = None;
            }
            Err(e) => *self.error.write().unwrap() = Some(format!("{e:#}")),
        }
        r.map(|_| ())
    }

    /// Whether the trusted certificate is missing or near its end.
    fn due(&self) -> bool {
        let Some(t) = &self.trusted else { return false };
        !self.certs.has_trusted() || super::cert::not_after(&t.cert).is_none_or(|end| (end - chrono::Utc::now()).num_days() < RENEW_DAYS)
    }

    /// Keep the certificates current while the console runs.
    pub fn keep(self: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                if self.due() {
                    match self.renew().await {
                        Ok(()) => tracing::info!("admin console: the trusted certificate for {} is issued", self.hostname),
                        Err(e) => tracing::warn!("admin console: the trusted certificate for {} could not be issued: {e:#}", self.hostname),
                    }
                } else if let Ok(k) = net::certified(&self.own.0, &self.own.1) {
                    // The organisation may have renewed its own in place.
                    self.certs.set_own(k);
                }
                tokio::time::sleep(EVERY).await;
            }
        });
    }
}

/// `tailscale cert`: tailscaled has Let's Encrypt issue the certificate for
/// the machine's name on the tailnet, answering the DNS-01 challenge
/// itself, and writes it and its key where asked. It needs HTTPS turned on
/// for the tailnet (its admin console, DNS), and the console's user allowed
/// to ask (`tailscale set --operator`).
async fn issue(t: &Issued, hostname: &str) -> Result<Arc<rustls::sign::CertifiedKey>> {
    let run = tokio::process::Command::new(&t.tailscale).arg("cert").arg("--cert-file").arg(&t.cert).arg("--key-file").arg(&t.key).arg(hostname).kill_on_drop(true).output();
    let out = tokio::time::timeout(Duration::from_secs(180), run)
        .await
        .context("tailscale did not finish within three minutes")?
        .with_context(|| format!("run {}", t.tailscale.display()))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        let said: Vec<&str> = said.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        let said = said[said.len().saturating_sub(4)..].join(" ");
        bail!("tailscale could not have it issued: {}", if said.is_empty() { format!("it ended with {}", out.status) } else { said });
    }
    net::certified(&t.cert, &t.key).context("tailscale wrote no usable certificate")
}
