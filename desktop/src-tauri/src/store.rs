//! What the app keeps: the appliance's address, pinned certificates, and the
//! session token. The token goes to the OS keyring (Secret Service on Linux,
//! Credential Manager on Windows); where there is no keyring it goes to a
//! file only this user can read. Nothing else secret is kept: provider keys
//! and tool credentials live in the appliance's vault.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

const SERVICE: &str = "magnus-desktop";

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Config {
    pub appliance: Option<String>,
    /// Certificate fingerprints by appliance address.
    #[serde(default)]
    pub pins: BTreeMap<String, String>,
}

pub struct Store {
    dir: PathBuf,
    cfg: Mutex<Config>,
}

fn write_private(path: &PathBuf, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let tmp = path.with_extension("tmp");
        let mut f = std::fs::OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(tmp, path)
    }
    #[cfg(not(unix))]
    {
        // The profile directory is private to the user on Windows.
        std::fs::write(path, bytes)
    }
}

impl Store {
    pub fn open() -> Store {
        let dir = dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("magnus");
        let _ = std::fs::create_dir_all(&dir);
        let cfg = std::fs::read(dir.join("app.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Store { dir, cfg: Mutex::new(cfg) }
    }

    pub fn config(&self) -> Config {
        self.cfg.lock().unwrap().clone()
    }

    fn save(&self, c: &Config) -> Result<(), String> {
        write_private(&self.dir.join("app.json"), &serde_json::to_vec_pretty(c).unwrap()).map_err(|e| format!("save settings: {e}"))
    }

    pub fn set_appliance(&self, url: &str) -> Result<(), String> {
        let mut c = self.cfg.lock().unwrap();
        c.appliance = Some(url.into());
        self.save(&c)
    }

    pub fn set_pin(&self, url: &str, fp: &str) -> Result<(), String> {
        let mut c = self.cfg.lock().unwrap();
        c.pins.insert(url.into(), fp.into());
        c.appliance = Some(url.into());
        self.save(&c)
    }

    fn token_file(&self, url: &str) -> PathBuf {
        use sha2::Digest;
        self.dir.join(format!("session-{}", &hex::encode(sha2::Sha256::digest(url.as_bytes()))[..16]))
    }

    pub fn token(&self, url: &str) -> Option<String> {
        if let Ok(e) = keyring::Entry::new(SERVICE, url) {
            if let Ok(t) = e.get_password() {
                return Some(t);
            }
        }
        std::fs::read_to_string(self.token_file(url)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }

    pub fn set_token(&self, url: &str, token: &str) -> Result<(), String> {
        let stored = keyring::Entry::new(SERVICE, url).and_then(|e| e.set_password(token)).is_ok();
        if stored && keyring::Entry::new(SERVICE, url).and_then(|e| e.get_password()).ok().as_deref() == Some(token) {
            let _ = std::fs::remove_file(self.token_file(url));
            return Ok(());
        }
        write_private(&self.token_file(url), token.as_bytes()).map_err(|e| format!("save the session: {e}"))
    }

    pub fn clear_token(&self, url: &str) {
        if let Ok(e) = keyring::Entry::new(SERVICE, url) {
            let _ = e.delete_credential();
        }
        let _ = std::fs::remove_file(self.token_file(url));
    }

    fn device_file(&self, url: &str) -> PathBuf {
        use sha2::Digest;
        self.dir.join(format!("device-{}", &hex::encode(sha2::Sha256::digest(url.as_bytes()))[..16]))
    }

    /// The certificate and key for a public address. The key is in a file
    /// only this user can read, not in the settings and not in the page.
    pub fn device(&self, url: &str) -> Option<(String, String)> {
        let b = std::fs::read(self.device_file(url)).ok()?;
        let v: serde_json::Value = serde_json::from_slice(&b).ok()?;
        let certificate = v["certificate"].as_str()?.to_string();
        let private_key = v["private_key"].as_str()?.to_string();
        if certificate.is_empty() || private_key.is_empty() { None } else { Some((certificate, private_key)) }
    }

    pub fn set_device(&self, url: &str, certificate: &str, private_key: &str) -> Result<(), String> {
        let v = serde_json::json!({ "certificate": certificate, "private_key": private_key });
        write_private(&self.device_file(url), &serde_json::to_vec(&v).unwrap()).map_err(|e| format!("save the device certificate: {e}"))
    }

    pub fn clear_device(&self, url: &str) {
        let _ = std::fs::remove_file(self.device_file(url));
    }
}
