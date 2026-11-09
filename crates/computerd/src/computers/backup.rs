//! Encrypted backups of a computer's volume. A backup is a list of chunks, and
//! chunks are named for their content under a per-computer key, so a later
//! backup stores only what changed.

use super::*;
use crate::seal::{self, KEY_BYTES};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::os::unix::fs::{FileExt, MetadataExt};
use std::time::{SystemTime, UNIX_EPOCH};

const BACKUP_CHUNK_BYTES: usize = 4 << 20;
const KEY_FILE: &str = "key";
const CHUNKS: &str = "chunks";
const LIST_SUFFIX: &str = ".list";
const SECONDS_PER_DAY: u64 = 86_400;
const SECRET_MODE: u32 = 0o600;

#[derive(Serialize, Deserialize)]
struct List {
    size: u64,
    /// By chunk number. A chunk of zeros has no entry.
    chunks: BTreeMap<u64, String>,
}

fn unix_seconds(t: SystemTime) -> Result<u64> {
    Ok(t.duration_since(UNIX_EPOCH).context("the clock is before 1970")?.as_secs())
}

fn write_new_secret(path: &Path, bytes: &[u8]) -> Result<()> {
    File::options().write(true).create_new(true).mode(SECRET_MODE).open(path).with_context(|| format!("create {}", path.display()))?.write_all(bytes)?;
    Ok(())
}

impl Computers {
    fn backups(&self, id: &ComputerId) -> PathBuf {
        self.state("backups").join(id.as_str())
    }

    fn backup_key(&self, id: &ComputerId) -> Result<[u8; KEY_BYTES]> {
        let dir = self.backups(id);
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir.join(CHUNKS)).context("make the backup directory")?;
        let path = dir.join(KEY_FILE);
        if !path.exists() {
            write_new_secret(&path, &seal::new_key()?)?;
        }
        std::fs::read(&path)?.try_into().map_err(|_| anyhow::anyhow!("a backup key is damaged"))
    }

    /// Backup lists, oldest first, with the second each was taken.
    fn lists(&self, id: &str) -> Result<Vec<(u64, PathBuf)>> {
        let mut found = vec![];
        let Ok(entries) = std::fs::read_dir(self.state("backups").join(id)) else { return Ok(found) };
        for e in entries {
            let path = e?.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if let Some(at) = name.strip_suffix(LIST_SUFFIX).and_then(|s| s.parse::<u64>().ok()) {
                found.push((at, path));
            }
        }
        found.sort();
        Ok(found)
    }

    /// Stores the volume as it is now. A running guest holds its filesystem
    /// still meanwhile; only the guest's own writes wait on that.
    pub fn backup(&self, id: &ComputerId) -> Result<Value> {
        let _one_at_a_time = self.backing_up.lock().unwrap_or_else(|e| e.into_inner());
        let Meta { uid, .. } = self.meta(id)?;
        let frozen = self.is_live(id) && !matches!(self.status(id)?, State::Paused);
        if frozen {
            let reply = self.ask_guest(id, uid, &json!({ "op": "freeze" }))?;
            ensure!(reply["ok"] == true, "the volume could not be held still: {}", reply["error"].as_str().unwrap_or("no reason given"));
        }
        let stored = self.store_volume(id);
        if frozen {
            self.ask_guest(id, uid, &json!({ "op": "thaw" }))?;
        }
        Ok(json!({ "at": stored?, "bytes": self.backup_bytes(id)? }))
    }

    fn store_volume(&self, id: &ComputerId) -> Result<u64> {
        let key = self.backup_key(id)?;
        let dir = self.backups(id);
        let mut volume = File::open(self.computer(id).join(VOLUME)).context("open the volume")?;
        let size = volume.metadata()?.len();
        let mut buf = vec![0u8; BACKUP_CHUNK_BYTES];
        let mut chunks = BTreeMap::new();
        let mut index = 0u64;
        loop {
            let mut filled = 0;
            while filled < buf.len() {
                match volume.read(&mut buf[filled..])? {
                    0 => break,
                    n => filled += n,
                }
            }
            if filled == 0 {
                break;
            }
            if buf[..filled].iter().any(|b| *b != 0) {
                let name = chunk_name(&key, &buf[..filled]);
                let path = dir.join(CHUNKS).join(&name);
                if !path.exists() {
                    let part = path.with_extension("part");
                    remove_file_if_there(&part)?;
                    write_new_secret(&part, &seal::seal_chunk(&key, &buf[..filled])?)?;
                    std::fs::rename(&part, &path)?;
                }
                chunks.insert(index, name);
            }
            index += 1;
        }
        let at = unix_seconds(SystemTime::now())?;
        let part = dir.join(format!("{at}.part"));
        remove_file_if_there(&part)?;
        write_new_secret(&part, &serde_json::to_vec(&List { size, chunks })?)?;
        std::fs::rename(&part, dir.join(format!("{at}{LIST_SUFFIX}"))).context("finish the backup")?;
        Ok(at)
    }

    fn backup_bytes(&self, id: &ComputerId) -> Result<u64> {
        let mut total = 0;
        for e in std::fs::read_dir(self.backups(id).join(CHUNKS))? {
            total += e?.metadata()?.len();
        }
        Ok(total)
    }

    /// Whether the volume has been written since its last backup.
    fn changed_since_backup(&self, id: &ComputerId) -> Result<bool> {
        let Some((at, _)) = self.lists(id.as_str())?.pop() else { return Ok(true) };
        let written = std::fs::metadata(self.computer(id).join(VOLUME))?.mtime();
        Ok(u64::try_from(written).unwrap_or(0) >= at)
    }

    /// Backs up what changed, for the service's own clock.
    pub fn backup_changed(&self) -> Result<()> {
        for id in self.list()? {
            if self.changed_since_backup(&id)? {
                self.backup(&id).with_context(|| format!("back up computer {}", id.as_str()))?;
            }
        }
        Ok(())
    }

    /// Puts the newest backup back as the volume of a computer that is not
    /// running. Whatever memory was saved no longer matches the disk.
    pub fn restore_backup(&self, id: &ComputerId) -> Result<Value> {
        let _lock = self.lock()?;
        let Meta { uid, .. } = self.meta(id)?;
        ensure!(!self.is_live(id), "computer {} is running", id.as_str());
        let Some((at, path)) = self.lists(id.as_str())?.pop() else { bail!("computer {} has no backup", id.as_str()) };
        let list: List = serde_json::from_slice(&std::fs::read(path)?).context("read the backup's list")?;
        let key = self.backup_key(id)?;
        let dir = self.computer(id);
        let part = dir.join("volume.part");
        remove_file_if_there(&part)?;
        let volume = File::options().write(true).create_new(true).mode(SECRET_MODE).open(&part).context("make the restored volume")?;
        volume.set_len(list.size)?;
        for (index, name) in &list.chunks {
            let plain = seal::open_chunk(&key, &std::fs::read(self.backups(id).join(CHUNKS).join(name))?)?;
            ensure!(chunk_name(&key, &plain) == *name, "a backup chunk has the wrong content");
            volume.write_all_at(&plain, index * BACKUP_CHUNK_BYTES as u64)?;
        }
        volume.sync_all()?;
        chown(&part, Some(uid), Some(uid)).context("give the VM its volume")?;
        std::fs::rename(&part, dir.join(VOLUME)).context("put the volume in place")?;
        self.discard_snapshot_locked(id)?;
        Ok(json!({ "restored": at }))
    }

    /// Nothing outlives the purge window: not a backup, the chunks only it
    /// used, or a snapshot. `now` is passed in so a test can move the clock.
    pub fn purge(&self, now: SystemTime) -> Result<()> {
        let now = unix_seconds(now)?;
        let oldest = now.saturating_sub(u64::from(self.cfg.purge_days) * SECONDS_PER_DAY);
        let root = self.state("backups");
        if let Ok(entries) = std::fs::read_dir(&root) {
            let _one_at_a_time = self.backing_up.lock().unwrap_or_else(|e| e.into_inner());
            for e in entries {
                let name = e?.file_name().to_string_lossy().into_owned();
                self.purge_backups(&name, oldest)?;
            }
        }
        let _lock = self.lock()?;
        for id in self.list()? {
            if self.snapshot_taken(&id).is_some_and(|at| at < oldest) {
                self.discard_snapshot_locked(&id)?;
            }
        }
        Ok(())
    }

    fn purge_backups(&self, id: &str, oldest: u64) -> Result<()> {
        let dir = self.state("backups").join(id);
        let lists = self.lists(id)?;
        let (expired, kept): (Vec<_>, Vec<_>) = lists.into_iter().partition(|(at, _)| *at < oldest);
        for (_, path) in &expired {
            std::fs::remove_file(path)?;
        }
        if kept.is_empty() {
            // A computer with no backup left has no use for its key.
            return remove_if_there(&dir);
        }
        let mut used = BTreeSet::new();
        for (_, path) in &kept {
            let list: List = serde_json::from_slice(&std::fs::read(path)?).context("read a backup's list")?;
            used.extend(list.chunks.into_values());
        }
        for e in std::fs::read_dir(dir.join(CHUNKS))? {
            let e = e?;
            if !used.contains(e.file_name().to_string_lossy().as_ref()) {
                std::fs::remove_file(e.path())?;
            }
        }
        Ok(())
    }
}

/// Every chunk stored for a computer, decrypted, one after another. For
/// checking what is kept, by hand or in a test: root only.
pub fn open_all_chunks(dir: &Path, to: &mut impl Write) -> Result<()> {
    let key: [u8; KEY_BYTES] = std::fs::read(dir.join(KEY_FILE))?.try_into().map_err(|_| anyhow::anyhow!("a backup key is damaged"))?;
    for e in std::fs::read_dir(dir.join(CHUNKS))? {
        to.write_all(&seal::open_chunk(&key, &std::fs::read(e?.path())?)?)?;
    }
    Ok(())
}

/// Keyed, so stored names say nothing about a chunk's content to someone
/// without the key.
fn chunk_name(key: &[u8; KEY_BYTES], plain: &[u8]) -> String {
    let digest = Sha256::new().chain_update(key).chain_update(plain).finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}
