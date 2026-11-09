//! A browser's profile, encrypted while its VM is stopped.
//!
//! The profile holds live sessions, so the plain volume exists only while the
//! VM runs. The key stays in Magnus's vault and is handed over per request.
use super::*;
use crate::seal::{self, KEY_BYTES};
use std::io::{BufReader, BufWriter, Write};

pub(super) const SEALED: &str = "volume.sealed";
/// Left by the first opening: the volume is a profile, and is secret.
pub(super) const PROFILE_MARK: &str = "profile";
const PART: &str = "volume.part";
const BUFFER_BYTES: usize = 1 << 20;
const VOLUME_MODE: u32 = 0o600;

fn write_whole(path: &Path, from: impl FnOnce(&mut BufWriter<File>) -> Result<()>) -> Result<()> {
    let file = File::options().write(true).create(true).truncate(true).mode(VOLUME_MODE).open(path).with_context(|| format!("create {}", path.display()))?;
    let mut to = BufWriter::with_capacity(BUFFER_BYTES, file);
    from(&mut to)?;
    to.flush()?;
    to.get_ref().sync_all().context("flush the file")
}

impl Computers {
    pub(super) fn is_profile(&self, id: &ComputerId) -> bool {
        self.computer(id).join(PROFILE_MARK).exists()
    }

    /// Safe to repeat: a volume already sealed is left alone.
    pub fn seal_volume(&self, id: &ComputerId, key: &[u8; KEY_BYTES]) -> Result<()> {
        let _one_at_a_time = self.sealing.lock().unwrap_or_else(|e| e.into_inner());
        self.meta(id)?;
        ensure!(!self.is_live(id), "computer {} is running", id.as_str());
        let dir = self.computer(id);
        let plain = dir.join(VOLUME);
        if !plain.exists() {
            ensure!(dir.join(SEALED).exists(), "computer {} has no volume", id.as_str());
            return Ok(());
        }
        let part = dir.join(PART);
        write_whole(&part, |to| seal::seal_again(key, &mut BufReader::with_capacity(BUFFER_BYTES, File::open(&plain)?), to))?;
        std::fs::rename(&part, dir.join(SEALED)).context("keep the sealed volume")?;
        std::fs::remove_file(&plain).context("remove the plain volume")
    }

    /// Safe to repeat: a volume already open is left alone.
    pub fn unseal_volume(&self, id: &ComputerId, key: &[u8; KEY_BYTES]) -> Result<()> {
        let _one_at_a_time = self.sealing.lock().unwrap_or_else(|e| e.into_inner());
        let meta = self.meta(id)?;
        let dir = self.computer(id);
        std::fs::write(dir.join(PROFILE_MARK), b"").context("mark the profile")?;
        let plain = dir.join(VOLUME);
        let sealed = dir.join(SEALED);
        if plain.exists() {
            let _ = std::fs::remove_file(&sealed);
            return Ok(());
        }
        let part = dir.join(PART);
        write_whole(&part, |to| seal::open_again(key, &mut BufReader::with_capacity(BUFFER_BYTES, File::open(&sealed)?), to).context("open the sealed volume"))?;
        chown(&part, Some(meta.uid), Some(meta.uid)).context("give the VM its volume")?;
        std::fs::rename(&part, &plain).context("put the volume back")?;
        std::fs::remove_file(&sealed).context("remove the sealed volume")
    }
}
