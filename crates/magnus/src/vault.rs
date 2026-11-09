//! The vault: secrets encrypted at rest under the state directory.
//!
//! Only two places read a secret: the boundary's credential stage, at the
//! moment it calls an app, and the model client, at the moment it calls an
//! adapter (K28). Nothing returns a secret over the API.

use anyhow::{Context, Result};
use base64::Engine;
use chacha20poly1305::{
    aead::{Aead, Payload},
    KeyInit, XChaCha20Poly1305, XNonce,
};
use rand::RngCore;
use std::{
    collections::BTreeMap,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};

pub struct Vault {
    dir: PathBuf,
    cipher: XChaCha20Poly1305,
    lock: Mutex<()>,
    /// Set while the kill switch is engaged: nothing is read or stored, so
    /// no secret leaves and no refreshed token comes back in (B11).
    sealed: std::sync::atomic::AtomicBool,
}

/// A secret value; formatting it never prints the value.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub(crate) fn new(s: String) -> Secret {
        Secret(s)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(..)")
    }
}

pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().create(true).truncate(true).write(true).mode(0o600).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// The mark of an entry encrypted bound to its name.
const BOUND: &str = "2:";
/// Present while the kill switch keeps the vault sealed, so a restart
/// finds it sealed whatever the log says.
const SEALED: &str = "sealed";
/// The sign-ins the kill switch took out of the vault and has not yet seen
/// revoked at their providers, as they were stored.
const REVOKING: &str = "revoking.json";

impl Vault {
    pub fn open(state_dir: &Path) -> Result<Vault> {
        let dir = state_dir.join("vault");
        std::fs::create_dir_all(&dir)?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        let key_path = dir.join("key");
        if !key_path.exists() {
            let mut k = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut k);
            write_private(&key_path, &k)?;
        }
        let key = std::fs::read(&key_path).context("read vault key")?;
        anyhow::ensure!(key.len() == 32, "vault key is corrupt");
        let cipher = XChaCha20Poly1305::new_from_slice(&key).map_err(|_| anyhow::anyhow!("vault key"))?;
        // Sealed by a kill switch that was not released: it stays sealed,
        // even if the switch was never recorded (a crash before the record).
        let sealed = dir.join(SEALED).exists();
        Ok(Vault { dir, cipher, lock: Mutex::new(()), sealed: std::sync::atomic::AtomicBool::new(sealed) })
    }

    /// Entries written before each was bound to its name (or restored from
    /// a backup of then), sealed again bound to it. Only the kernel that
    /// owns the appliance does this: opening the vault changes nothing, so
    /// a new release's check before an update leaves the running one's
    /// vault as it is.
    pub fn bind_unbound(&self) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        let mut all = self.read_all()?;
        let unbound: Vec<String> = all.iter().filter(|(_, v)| !v.starts_with(BOUND)).map(|(n, _)| n.clone()).collect();
        if unbound.is_empty() {
            return Ok(());
        }
        for n in unbound {
            let blob = base64::engine::general_purpose::STANDARD.decode(&all[&n])?;
            anyhow::ensure!(blob.len() > 24, "vault entry {n} is corrupt");
            let pt = self.cipher.decrypt(XNonce::from_slice(&blob[..24]), &blob[24..]).map_err(|_| anyhow::anyhow!("vault entry {n} does not decrypt"))?;
            all.insert(n.clone(), self.seal_as(&n, &pt)?);
        }
        write_private(&self.path(), &serde_json::to_vec(&all)?)
    }

    /// Every entry sealed again as releases before binding read it, for
    /// going back to an earlier release: the release gone back to binds
    /// them again when it starts, if it knows how. The vault stays sealed
    /// until then, so nothing is stored bound in between.
    pub fn unbind_all(&self) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        self.sealed.store(true, std::sync::atomic::Ordering::SeqCst);
        let mut all = self.read_all()?;
        for (n, v) in all.iter_mut() {
            let Some(b64) = v.strip_prefix(BOUND) else { continue };
            let blob = base64::engine::general_purpose::STANDARD.decode(b64)?;
            anyhow::ensure!(blob.len() > 24, "vault entry {n} is corrupt");
            let pt = self
                .cipher
                .decrypt(XNonce::from_slice(&blob[..24]), Payload { msg: &blob[24..], aad: n.as_bytes() })
                .map_err(|_| anyhow::anyhow!("vault entry {n} does not decrypt"))?;
            let mut nonce = [0u8; 24];
            rand::rngs::OsRng.fill_bytes(&mut nonce);
            let mut out = nonce.to_vec();
            out.extend(self.cipher.encrypt(XNonce::from_slice(&nonce), pt.as_slice()).map_err(|_| anyhow::anyhow!("encrypt"))?);
            *v = base64::engine::general_purpose::STANDARD.encode(out);
        }
        write_private(&self.path(), &serde_json::to_vec(&all)?)
    }

    /// A value encrypted for the entry `name` alone: its name is the
    /// associated data, so under any other name it does not decrypt
    /// (connectors spec, Custody: bound to one user and one connector).
    fn seal_as(&self, name: &str, value: &[u8]) -> Result<String> {
        let mut nonce = [0u8; 24];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let ct = self.cipher.encrypt(XNonce::from_slice(&nonce), Payload { msg: value, aad: name.as_bytes() }).map_err(|_| anyhow::anyhow!("encrypt"))?;
        let mut blob = nonce.to_vec();
        blob.extend(ct);
        Ok(format!("{BOUND}{}", base64::engine::general_purpose::STANDARD.encode(blob)))
    }

    fn path(&self) -> PathBuf {
        self.dir.join("secrets.json")
    }

    fn read_all(&self) -> Result<BTreeMap<String, String>> {
        match std::fs::read(self.path()) {
            Ok(b) => Ok(serde_json::from_slice(&b)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn put(&self, name: &str, value: &str) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        anyhow::ensure!(!self.is_sealed(), "the kill switch is engaged; nothing is stored in the vault");
        let mut all = self.read_all()?;
        all.insert(name.into(), self.seal_as(name, value.as_bytes())?);
        write_private(&self.path(), &serde_json::to_vec(&all)?)
    }

    pub fn get(&self, name: &str) -> Result<Option<Secret>> {
        let _g = self.lock.lock().unwrap();
        if self.is_sealed() {
            return Ok(None);
        }
        self.get_locked(name)
    }

    fn get_locked(&self, name: &str) -> Result<Option<Secret>> {
        let all = self.read_all()?;
        let Some(stored) = all.get(name) else { return Ok(None) };
        self.open_as(name, stored).map(Some)
    }

    /// An entry as stored, decrypted for its name.
    fn open_as(&self, name: &str, stored: &str) -> Result<Secret> {
        let b64 = stored.strip_prefix(BOUND).ok_or_else(|| anyhow::anyhow!("vault entry {name} is not bound to its name"))?;
        let blob = base64::engine::general_purpose::STANDARD.decode(b64)?;
        anyhow::ensure!(blob.len() > 24, "vault entry {name} is corrupt");
        let pt = self
            .cipher
            .decrypt(XNonce::from_slice(&blob[..24]), Payload { msg: &blob[24..], aad: name.as_bytes() })
            .map_err(|_| anyhow::anyhow!("vault entry {name} does not decrypt"))?;
        Ok(Secret(String::from_utf8(pt)?))
    }

    pub fn names(&self) -> Result<Vec<String>> {
        let _g = self.lock.lock().unwrap();
        Ok(self.read_all()?.into_keys().collect())
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn unseal(&self) {
        let _g = self.lock.lock().unwrap();
        let _ = std::fs::remove_file(self.dir.join(SEALED));
        self.sealed.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Seal the vault and take the secrets named in `take` out of it,
    /// returning them once (for revocation at the provider). The rest stay,
    /// unreadable until the vault is unsealed.
    ///
    /// Those taken wait in the vault, sealed, until `revoked` says each was
    /// revoked: first written there, then removed from the rest, so a crash
    /// at any point leaves each one in one place or the other, never lost
    /// unrevoked. With those an earlier switch took and did not see revoked.
    pub fn seal_and_take(&self, take: &[String]) -> Result<Vec<(String, Secret)>> {
        let _g = self.lock.lock().unwrap();
        self.seal_locked();
        let mut all = self.read_all()?;
        let mut pending = self.read_pending()?;
        for n in take {
            if let Some(v) = all.get(n) {
                pending.insert(n.clone(), v.clone());
            }
        }
        write_private(&self.dir.join(REVOKING), &serde_json::to_vec(&pending)?)?;
        all.retain(|n, _| !take.contains(n));
        write_private(&self.path(), &serde_json::to_vec(&all)?)?;
        self.pending_locked(&pending)
    }

    /// The sign-ins taken out to be revoked and not yet seen revoked.
    pub fn revoking(&self) -> Result<Vec<(String, Secret)>> {
        let _g = self.lock.lock().unwrap();
        let pending = self.read_pending()?;
        self.pending_locked(&pending)
    }

    /// The provider answered for this sign-in (revoked or not): it no longer
    /// waits.
    pub fn revoked(&self, name: &str) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        let mut pending = self.read_pending()?;
        if pending.remove(name).is_some() {
            write_private(&self.dir.join(REVOKING), &serde_json::to_vec(&pending)?)?;
        }
        Ok(())
    }

    fn read_pending(&self) -> Result<BTreeMap<String, String>> {
        match std::fs::read(self.dir.join(REVOKING)) {
            Ok(b) => Ok(serde_json::from_slice(&b)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e.into()),
        }
    }

    fn pending_locked(&self, pending: &BTreeMap<String, String>) -> Result<Vec<(String, Secret)>> {
        let mut out = vec![];
        for (n, stored) in pending {
            out.push((n.clone(), self.open_as(n, stored)?));
        }
        Ok(out)
    }

    fn seal_locked(&self) {
        // Best effort: sealed in memory whatever the disk says.
        let _ = write_private(&self.dir.join(SEALED), b"");
        self.sealed.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// The vault's key and its encrypted contents as stored, for a backup.
    pub fn raw(&self) -> Result<(Vec<u8>, String)> {
        let _g = self.lock.lock().unwrap();
        let key = std::fs::read(self.dir.join("key"))?;
        let secrets = std::fs::read_to_string(self.path()).unwrap_or_else(|_| "{}".into());
        Ok((key, secrets))
    }

    /// Remove a secret (a connector the owner removed).
    pub fn remove(&self, name: &str) -> Result<Option<Secret>> {
        let _g = self.lock.lock().unwrap();
        let mut all = self.read_all()?;
        let old = self.get_locked(name)?;
        all.remove(name);
        write_private(&self.path(), &serde_json::to_vec(&all)?)?;
        Ok(old)
    }

    /// Seal without emptying: the kill switch, before it takes anything, or
    /// a restart while it is engaged.
    pub fn seal(&self) {
        let _g = self.lock.lock().unwrap();
        self.seal_locked();
    }
}

/// That every entry of a vault's stored contents opens under `key`, as
/// the vault would open it (a backup's restore test).
pub fn check(key: &[u8], secrets: &str) -> Result<()> {
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| anyhow::anyhow!("the backed-up vault key is corrupt"))?;
    let all: BTreeMap<String, String> = serde_json::from_str(secrets)?;
    for (name, v) in &all {
        // Bound to its name, or from before binding (bound when it opens).
        let (b64, aad) = match v.strip_prefix(BOUND) {
            Some(b) => (b, name.as_bytes()),
            None => (v.as_str(), &[][..]),
        };
        let blob = base64::engine::general_purpose::STANDARD.decode(b64)?;
        anyhow::ensure!(blob.len() > 24, "secret {name} is corrupt");
        cipher.decrypt(XNonce::from_slice(&blob[..24]), Payload { msg: &blob[24..], aad }).map_err(|_| anyhow::anyhow!("secret {name} does not decrypt"))?;
    }
    Ok(())
}

/// Replace every encoding of `secret` in `text` (raw, base64, URL-safe base64,
/// hex) so that nothing a careless app echoes back carries it further.
pub fn redact(text: &str, secret: &str) -> String {
    if secret.len() < 4 {
        return text.to_string();
    }
    let b = secret.as_bytes();
    let forms = [
        secret.to_string(),
        base64::engine::general_purpose::STANDARD.encode(b),
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(b),
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b),
        hex::encode(b),
        hex::encode_upper(b),
    ];
    let mut out = text.to_string();
    for f in forms.iter() {
        if !f.is_empty() {
            out = out.replace(f.as_str(), "[credential removed]");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(dir: &Path) -> BTreeMap<String, String> {
        serde_json::from_slice(&std::fs::read(dir.join("vault/secrets.json")).unwrap()).unwrap()
    }

    #[test]
    fn a_sign_in_moved_under_another_name_does_not_open() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        v.put("connector_calendar__alice", "alice-token").unwrap();
        v.put("connector_calendar__bob", "bob-token").unwrap();
        // Bob's entry copied over Alice's, as someone with the file might.
        let mut all = stored(dir.path());
        all.insert("connector_calendar__alice".into(), all["connector_calendar__bob"].clone());
        write_private(&dir.path().join("vault/secrets.json"), &serde_json::to_vec(&all).unwrap()).unwrap();
        let v = Vault::open(dir.path()).unwrap();
        assert!(v.get("connector_calendar__alice").is_err(), "Bob's sign-in opened as Alice's");
        assert_eq!(v.get("connector_calendar__bob").unwrap().unwrap().expose(), "bob-token");
    }

    /// An entry as releases before binding wrote it: no associated data, no mark.
    fn legacy(v: &Vault, value: &[u8]) -> String {
        let mut nonce = [0u8; 24];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let mut blob = nonce.to_vec();
        blob.extend(v.cipher.encrypt(XNonce::from_slice(&nonce), value).unwrap());
        base64::engine::general_purpose::STANDARD.encode(blob)
    }

    /// How releases before binding read an entry.
    fn read_as_before(v: &Vault, stored: &str) -> Option<String> {
        let blob = base64::engine::general_purpose::STANDARD.decode(stored).ok()?;
        v.cipher.decrypt(XNonce::from_slice(&blob[..24]), &blob[24..]).ok().and_then(|p| String::from_utf8(p).ok())
    }

    #[test]
    fn an_entry_from_before_binding_is_bound_only_when_the_kernel_takes_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        let all = BTreeMap::from([("connector_calendar__alice".to_string(), legacy(&v, b"old-token"))]);
        write_private(&dir.path().join("vault/secrets.json"), &serde_json::to_vec(&all).unwrap()).unwrap();
        // A new release's check before an update opens the running release's vault.
        let checked = Vault::open(dir.path()).unwrap();
        assert_eq!(checked.names().unwrap(), vec!["connector_calendar__alice".to_string()]);
        assert_eq!(read_as_before(&v, &stored(dir.path())["connector_calendar__alice"]).as_deref(), Some("old-token"), "opening the vault changed it under the running release");
        let owned = Vault::open(dir.path()).unwrap();
        owned.bind_unbound().unwrap();
        assert_eq!(owned.get("connector_calendar__alice").unwrap().unwrap().expose(), "old-token");
        assert!(stored(dir.path())["connector_calendar__alice"].starts_with(BOUND), "the old entry was not bound when the kernel took the vault");
    }

    #[test]
    fn going_back_a_release_leaves_the_vault_as_the_earlier_release_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        v.put("connector_calendar__alice", "alice-token").unwrap();
        v.unbind_all().unwrap();
        assert_eq!(read_as_before(&v, &stored(dir.path())["connector_calendar__alice"]).as_deref(), Some("alice-token"));
        assert!(v.put("connector_calendar__bob", "bob-token").is_err(), "a sign-in was stored bound after the vault was unbound for the earlier release");
        // The release gone back to, when it binds, binds them again.
        let again = Vault::open(dir.path()).unwrap();
        again.bind_unbound().unwrap();
        assert_eq!(again.get("connector_calendar__alice").unwrap().unwrap().expose(), "alice-token");
    }

    /// What the kill switch takes waits in the vault until revoked, and the
    /// seal outlives a restart until the switch is released.
    #[test]
    fn a_taken_sign_in_waits_until_revoked_and_the_seal_outlives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        v.put("connector_calendar__alice", "alice-token").unwrap();
        v.put("app_key", "key").unwrap();
        let taken = v.seal_and_take(&["connector_calendar__alice".into()]).unwrap();
        assert_eq!(taken.iter().map(|(n, s)| (n.as_str(), s.expose())).collect::<Vec<_>>(), vec![("connector_calendar__alice", "alice-token")]);
        // A crash here: sealed again on opening, the sign-in still to revoke.
        let v = Vault::open(dir.path()).unwrap();
        assert!(v.is_sealed(), "a restart unsealed the vault");
        assert_eq!(v.revoking().unwrap().len(), 1, "the taken sign-in was lost before it was revoked");
        assert!(v.get("connector_calendar__alice").unwrap().is_none());
        v.revoked("connector_calendar__alice").unwrap();
        assert!(v.revoking().unwrap().is_empty());
        v.unseal();
        let v = Vault::open(dir.path()).unwrap();
        assert!(!v.is_sealed(), "released, the vault stayed sealed");
        assert_eq!(v.get("app_key").unwrap().unwrap().expose(), "key");
    }
}
