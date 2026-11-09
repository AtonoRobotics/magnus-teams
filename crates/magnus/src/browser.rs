//! The browser: Chromium in a jail on the host (`browser/jail.sh`), driven
//! by a sidecar process, one isolated context per run. Only the boundary calls it; its output is untrusted.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// The browser sidecar's settings (`[browser]`).
#[derive(Deserialize, Clone, Debug)]
pub struct Settings {
    pub command: Vec<String>,
    pub chromium: PathBuf,
    /// The jail's bubblewrap, from `config/install-browser-jail.sh`.
    #[serde(default = "default_bwrap")]
    pub bwrap: PathBuf,
    /// Internal sites the browser may open: a name or address, with or
    /// without `:port`, or a range (`10.1.0.0/16`). Every other loopback,
    /// link-local, private or reserved address is refused (#185).
    #[serde(default)]
    pub allow: Vec<String>,
}

fn default_bwrap() -> PathBuf {
    PathBuf::from("/usr/local/lib/magnus/bwrap")
}

pub struct Browser {
    cfg: Option<Settings>,
    socket: PathBuf,
    child: Mutex<Option<Child>>,
    start: tokio::sync::Mutex<()>,
}

impl Browser {
    pub fn new(cfg: Option<Settings>, state_dir: &std::path::Path) -> Browser {
        Browser { cfg, socket: state_dir.join("browser.sock"), child: Mutex::new(None), start: tokio::sync::Mutex::new(()) }
    }

    pub fn available(&self) -> bool {
        self.cfg.is_some()
    }

    async fn ensure(&self) -> Result<()> {
        let _g = self.start.lock().await;
        let alive = self.child.lock().unwrap().as_mut().is_some_and(|c| c.try_wait().ok().flatten().is_none());
        if alive {
            return Ok(());
        }
        let cfg = self.cfg.as_ref().ok_or_else(|| anyhow!("no browser is configured on this appliance"))?;
        let _ = std::fs::remove_file(&self.socket);
        let mut cmd = Command::new(&cfg.command[0]);
        cmd.args(&cfg.command[1..]).arg("--socket").arg(&self.socket);
        for site in &cfg.allow {
            cmd.arg("--allow").arg(site);
        }
        cmd.env_clear().env("PATH", std::env::var("PATH").unwrap_or_default()).env("MAGNUS_CHROMIUM", &cfg.chromium).env("MAGNUS_BWRAP", &cfg.bwrap);
        if let Ok(h) = std::env::var("HOME") {
            cmd.env("HOME", h);
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let child = crate::spawn::spawn(cmd).context("start the browser sidecar")?;
        *self.child.lock().unwrap() = Some(child);
        for _ in 0..100 {
            if tokio::net::UnixStream::connect(&self.socket).await.is_ok() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(anyhow!("the browser sidecar did not start"))
    }

    pub async fn request(&self, run: &str, mut req: Value) -> Result<Value> {
        self.ensure().await?;
        req["run"] = json!(run);
        let s = tokio::net::UnixStream::connect(&self.socket).await?;
        let (r, mut w) = s.into_split();
        w.write_all(format!("{req}\n").as_bytes()).await?;
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(90), BufReader::new(r).read_line(&mut line)).await.map_err(|_| anyhow!("the browser did not answer in time"))??;
        let v: Value = serde_json::from_str(&line).map_err(|_| anyhow!("the browser sent an unreadable answer"))?;
        if let Some(e) = v["error"].as_str() {
            return Err(anyhow!("{e}"));
        }
        Ok(v)
    }

    pub async fn close(&self, run: &str) {
        if self.cfg.is_some() && self.child.lock().unwrap().is_some() {
            let _ = self.request(run, json!({ "op": "close" })).await;
        }
    }

    /// Close every run's pages and the browser with them (the kill switch).
    /// A run that goes on later gets a new browser and a blank page.
    pub async fn close_all(&self) {
        if self.cfg.is_some() && self.child.lock().unwrap().is_some() {
            if let Err(e) = self.request("", json!({ "op": "close_all" })).await {
                tracing::error!("closing the browser: {e:#}");
            }
        }
    }
}
