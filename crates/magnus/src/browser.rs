//! The browser: Chromium in a jail on the host (`browser/jail.sh`), driven
//! by a sidecar process, one isolated context per run. Only the boundary calls it; its output is untrusted.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// How often, and how many times, a starting sidecar's socket is tried.
const START_POLL: Duration = Duration::from_millis(100);
const START_TRIES: u32 = 100;
/// The longest one browser request may take to answer.
const ANSWER_WAIT: Duration = Duration::from_secs(90);

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

/// Starts the jail the way the browser will, with nothing inside it. Run by
/// `magnus check`, so a release that needs a jail this machine lacks is not
/// switched to: updating runs without root and cannot install one.
pub fn check_jail(bwrap: &Path) -> Result<()> {
    const PROBE: [&str; 12] = ["--unshare-all", "--ro-bind", "/usr", "/usr", "--symlink", "usr/lib", "/lib", "--symlink", "usr/lib64", "/lib64", "--", "/usr/bin/true"];
    let fix = "run install-browser-jail.sh from the release with sudo, naming the user Magnus runs as, then restart Magnus";
    let out = Command::new(bwrap).args(PROBE).output().with_context(|| format!("this user cannot run the browser jail {}: {fix}", bwrap.display()))?;
    if !out.status.success() {
        return Err(anyhow!("the browser jail {} does not start ({}): {fix}", bwrap.display(), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

pub struct Browser {
    cfg: Option<Settings>,
    socket: PathBuf,
    child: Mutex<Option<Child>>,
    start: tokio::sync::Mutex<()>,
    host: crate::spawn::HostEnv,
}

impl Browser {
    pub fn new(cfg: Option<Settings>, state_dir: &std::path::Path, host: crate::spawn::HostEnv) -> Browser {
        Browser { cfg, socket: state_dir.join("browser.sock"), child: Mutex::new(None), start: tokio::sync::Mutex::new(()), host }
    }

    pub fn available(&self) -> bool {
        self.cfg.is_some()
    }

    async fn ensure(&self) -> Result<()> {
        let _g = self.start.lock().await;
        let alive = self.child.lock().unwrap_or_else(|e| e.into_inner()).as_mut().is_some_and(|c| c.try_wait().ok().flatten().is_none());
        if alive {
            return Ok(());
        }
        let cfg = self.cfg.as_ref().ok_or_else(|| anyhow!("no browser is configured on this server"))?;
        let _ = std::fs::remove_file(&self.socket);
        let mut cmd = Command::new(&cfg.command[0]);
        cmd.args(&cfg.command[1..]).arg("--socket").arg(&self.socket);
        for site in &cfg.allow {
            cmd.arg("--allow").arg(site);
        }
        cmd.env_clear().env("PATH", &self.host.path).env("MAGNUS_CHROMIUM", &cfg.chromium).env("MAGNUS_BWRAP", &cfg.bwrap);
        if let Some(h) = &self.host.home {
            cmd.env("HOME", h);
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let child = crate::spawn::spawn(cmd).context("start the browser sidecar")?;
        *self.child.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
        for _ in 0..START_TRIES {
            if tokio::net::UnixStream::connect(&self.socket).await.is_ok() {
                return Ok(());
            }
            tokio::time::sleep(START_POLL).await;
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
        tokio::time::timeout(ANSWER_WAIT, BufReader::new(r).read_line(&mut line)).await.map_err(|_| anyhow!("the browser did not answer in time"))??;
        let v: Value = serde_json::from_str(&line).map_err(|_| anyhow!("the browser sent an unreadable answer"))?;
        if let Some(e) = v["error"].as_str() {
            return Err(anyhow!("{e}"));
        }
        Ok(v)
    }

    pub async fn close(&self, run: &str) {
        if self.cfg.is_some() && self.child.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
            let _ = self.request(run, json!({ "op": "close" })).await;
        }
    }

    /// Close every run's pages and the browser with them (the kill switch).
    /// A run that goes on later gets a new browser and a blank page.
    pub async fn close_all(&self) {
        if self.cfg.is_some() && self.child.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
            if let Err(e) = self.request("", json!({ "op": "close_all" })).await {
                tracing::error!("closing the browser: {e:#}");
            }
        }
    }
}

#[cfg(test)]
mod jail_tests {
    use super::check_jail;
    use std::path::Path;

    #[test]
    fn a_release_is_refused_where_the_jail_cannot_start() {
        assert!(check_jail(Path::new("/nonexistent/bwrap")).is_err(), "a missing jail passed");
        assert!(check_jail(Path::new("/usr/bin/false")).is_err(), "a jail that fails to start passed");
        assert!(check_jail(Path::new("/usr/bin/true")).is_ok());
    }
}
