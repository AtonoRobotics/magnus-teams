//! The app manifest: what a computer has installed, kept outside it with its
//! installers, so a VM can be thrown away and its apps put back.

use super::{client, App, Change, Status};
use crate::config::Person;
use crate::controls::Refused;
use crate::kernel::Kernel;
use anyhow::{bail, ensure, Context, Result};
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const INSTALLERS: &str = "computer-installers";
const IN_GUEST: &str = "/var/tmp/magnus-installers";
const CHUNK_BYTES: u64 = 16 << 20;
const MAX_INSTALLER_BYTES: u64 = 4 << 30;
const STEP_WAIT: Duration = Duration::from_secs(1800);
/// Seconds, for the guest's own limit on a command.
const STEP_SECONDS: u64 = 1800;
const MAX_FIELD_CHARS: usize = 500;
const MAX_STEPS: usize = 32;
const INSTALLER_MODE: u32 = 0o600;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewApp {
    name: String,
    version: String,
    source: String,
    /// Where the installer is on the computer.
    installer: String,
    steps: Vec<String>,
    #[serde(default)]
    hosts: Vec<String>,
}

fn checked_fields(name: &str, version: &str, source: &str, steps: &[String]) -> Result<(), Refused> {
    let ok = |s: &str| !s.trim().is_empty() && s.chars().count() <= MAX_FIELD_CHARS;
    if !(ok(name) && ok(version) && ok(source)) {
        return Err(Refused::Invalid(format!("an app has a name, version and source of 1 to {MAX_FIELD_CHARS} characters")));
    }
    if steps.is_empty() || steps.len() > MAX_STEPS || !steps.iter().all(|s| ok(s)) {
        return Err(Refused::Invalid(format!("an app has 1 to {MAX_STEPS} install steps of 1 to {MAX_FIELD_CHARS} characters")));
    }
    Ok(())
}

fn file_in_cache(dir: &Path, sha256: &str) -> Result<PathBuf> {
    ensure!(sha256.len() == 64 && sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "not a sha-256");
    Ok(dir.join(sha256))
}

/// The cached installer, read only if it is still what the manifest says.
fn verified(dir: &Path, sha256: &str) -> Result<PathBuf> {
    let path = file_in_cache(dir, sha256)?;
    let mut file = std::fs::File::open(&path).with_context(|| format!("no cached installer {sha256}"))?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        match file.read(&mut buf)? {
            0 => break,
            n => hash.update(&buf[..n]),
        }
    }
    let got = hex::encode(hash.finalize());
    ensure!(got == sha256, "the cached installer does not match its manifest entry: {got}");
    Ok(path)
}

async fn exec(socket: &Path, id: &str, command: String) -> Result<Value> {
    client::guest_for(socket, id, json!({ "op": "exec", "command": command, "timeout_s": STEP_SECONDS }), STEP_WAIT).await
}

/// Puts one app back on a computer whose overlay is new.
pub(super) async fn replay(dir: &Path, socket: &Path, id: &str, app: &App) -> Result<()> {
    let cached = verified(dir, &app.sha256)?;
    let target = format!("{IN_GUEST}/{}", app.sha256);
    let mut file = std::fs::File::open(cached)?;
    let mut buf = vec![0u8; CHUNK_BYTES as usize];
    let mut append = false;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 && append {
            break;
        }
        let data_b64 = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
        let reply = client::guest(socket, id, json!({ "op": "write", "path": target, "data_b64": data_b64, "append": append })).await?;
        ensure!(reply["error"].is_null(), "{}", reply["error"]);
        if n == 0 {
            break;
        }
        append = true;
    }
    for step in &app.steps {
        let out = exec(socket, id, format!("INSTALLER={target}; export INSTALLER; {step}")).await?;
        if out["exit_code"] != 0 {
            bail!("the step `{step}` exited with {}: {}", out["exit_code"], out["stderr"].as_str().unwrap_or_default());
        }
    }
    exec(socket, id, format!("rm -f {target}; sync")).await?;
    Ok(())
}

impl Kernel {
    pub(super) fn computer_installers(&self) -> PathBuf {
        self.cfg.appliance.state_dir.join(INSTALLERS)
    }

    pub fn computer_apps(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        let c = self.controlled_computer(who, id)?;
        Ok(json!({ "apps": c.apps, "replay_error": c.replay_error }))
    }

    /// Caches the installer from the computer, then writes the manifest entry.
    pub async fn record_computer_app(&self, who: &Person, id: &str, new: NewApp) -> Result<Value, Refused> {
        let NewApp { name, version, source, installer, steps, hosts } = new;
        checked_fields(&name, &version, &source, &steps)?;
        let socket = self.computer_socket()?;
        let c = self.controlled_computer(who, id)?;
        if c.status != Status::Running {
            return Err(Refused::Conflict("an app is recorded from a running computer".into()));
        }
        let sha256 = self.cache_installer(&socket, id, &installer).await.map_err(|e| Refused::Failed(format!("{e:#}")))?;
        let app = App { name, version, source, sha256: sha256.clone(), steps, hosts };
        self.record_computer(who, Change::AppInstalled { id: id.into(), app }).await?;
        Ok(json!({ "sha256": sha256 }))
    }

    pub async fn remove_computer_app(&self, who: &Person, id: &str, name: &str) -> Result<Value, Refused> {
        let c = self.controlled_computer(who, id)?;
        if !c.apps.iter().any(|a| a.name == name) {
            return Err(Refused::NotFound("app"));
        }
        self.record_computer(who, Change::AppRemoved { id: id.into(), name: name.into() }).await?;
        Ok(json!({ "removed": name }))
    }

    /// Hashed here as it arrives, so the hash is Magnus's own measure and not
    /// the guest's word.
    async fn cache_installer(&self, socket: &Path, id: &str, installer: &str) -> Result<String> {
        let dir = self.computer_installers();
        tokio::fs::create_dir_all(&dir).await.context("make the installer cache")?;
        let part = dir.join(format!("part-{id}"));
        let _ = tokio::fs::remove_file(&part).await;
        let mut out = tokio::fs::OpenOptions::new().write(true).create_new(true).mode(INSTALLER_MODE).open(&part).await.context("start caching an installer")?;
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut hash = Sha256::new();
        let mut offset = 0u64;
        loop {
            let reply = client::guest(socket, id, json!({ "op": "read", "path": installer, "offset": offset, "length": CHUNK_BYTES })).await?;
            ensure!(reply["error"].is_null(), "{}", reply["error"]);
            let bytes = b64.decode(reply["data_b64"].as_str().unwrap_or_default()).context("the computer sent bad base64")?;
            if bytes.is_empty() {
                break;
            }
            offset += bytes.len() as u64;
            ensure!(offset <= MAX_INSTALLER_BYTES, "an installer is at most {MAX_INSTALLER_BYTES} bytes");
            hash.update(&bytes);
            out.write_all(&bytes).await?;
        }
        ensure!(offset > 0, "the installer is empty");
        out.flush().await?;
        let sha256 = hex::encode(hash.finalize());
        tokio::fs::rename(&part, file_in_cache(&dir, &sha256)?).await.context("finish caching an installer")?;
        Ok(sha256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cached_installer_is_used_only_if_it_still_matches() {
        let dir = tempfile::tempdir().unwrap();
        let sha = hex::encode(Sha256::digest(b"installer"));
        std::fs::write(dir.path().join(&sha), b"installer").unwrap();
        assert!(verified(dir.path(), &sha).is_ok());
        std::fs::write(dir.path().join(&sha), b"tampered!").unwrap();
        assert!(verified(dir.path(), &sha).is_err(), "a changed installer was accepted");
        for bad in ["", "../../etc/passwd", &"z".repeat(64), &"A".repeat(64)] {
            assert!(verified(dir.path(), bad).is_err(), "{bad} was accepted as a hash");
        }
    }

    #[test]
    fn an_app_entry_has_its_fields() {
        let steps = vec!["dpkg -i $INSTALLER".to_string()];
        assert!(checked_fields("a", "1", "https://x", &steps).is_ok());
        assert!(checked_fields("", "1", "s", &steps).is_err());
        assert!(checked_fields("a", "1", "s", &[]).is_err());
        assert!(checked_fields("a", "1", "s", &vec!["x".to_string(); MAX_STEPS + 1]).is_err());
    }
}
