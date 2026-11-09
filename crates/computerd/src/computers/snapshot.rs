//! Hibernating to an encrypted snapshot and waking from it. The VM's memory
//! is opaque here: root encrypts and decrypts it and never reads it.

use super::*;
use crate::relay::{SNAPSHOT_MEMORY, SNAPSHOT_STATE};
use crate::seal::{self, KEY_BYTES};
use base64::Engine;
use std::io::{BufReader, BufWriter, Read};
use std::os::unix::fs::MetadataExt;
use std::time::{SystemTime, UNIX_EPOCH};

const SNAPSHOT_DIR: &str = "snapshot";
const KEY_FILE: &str = "key";
/// Set when a snapshot is taken: it is valid only on this base image.
const BASE_FILE: &str = "base";
const BUFFER_BYTES: usize = 1 << 20;
const RESET_ENTROPY_BYTES: usize = 64;
const MACHINE_ID_BYTES: usize = 16;
const HOST_SUFFIX_BYTES: usize = 8;
const FILES: [&str; 2] = [SNAPSHOT_STATE, SNAPSHOT_MEMORY];

impl Computers {
    fn snapshot_dir(&self, id: &ComputerId) -> PathBuf {
        self.computer(id).join(SNAPSHOT_DIR)
    }

    /// When the snapshot was taken, in seconds since 1970.
    pub(super) fn snapshot_taken(&self, id: &ComputerId) -> Option<u64> {
        let modified = std::fs::metadata(self.snapshot_dir(id).join(BASE_FILE)).ok()?.modified().ok()?;
        Some(modified.duration_since(UNIX_EPOCH).ok()?.as_secs())
    }

    pub(super) fn has_snapshot(&self, id: &ComputerId) -> bool {
        self.snapshot_dir(id).join(BASE_FILE).exists()
    }

    pub(super) fn discard_snapshot_locked(&self, id: &ComputerId) -> Result<()> {
        remove_if_there(&self.snapshot_dir(id))
    }

    /// An edit made while hibernated makes the memory stale.
    pub fn discard_snapshot(&self, id: &ComputerId) -> Result<()> {
        let _lock = self.lock()?;
        self.meta(id)?;
        self.discard_snapshot_locked(id)
    }

    fn run_as_vm(&self, id: &ComputerId, uid: u32, what: &str) -> Result<()> {
        let out = Command::new(std::env::current_exe().context("find this program")?)
            .env_clear()
            .arg("api")
            .arg(self.jail_root(id))
            .arg(uid.to_string())
            .arg(what)
            .output()
            .context("reach the VM")?;
        ensure!(out.status.success(), "{what} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
        Ok(())
    }

    /// The four things a snapshot would otherwise hand to every restore. The
    /// network identity is the host name until a computer has a network card.
    fn reset_guest(&self, id: &ComputerId, uid: u32) -> Result<()> {
        let mut entropy = [0u8; RESET_ENTROPY_BYTES];
        File::open("/dev/urandom").context("open the system's randomness")?.read_exact(&mut entropy)?;
        let unix_seconds = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let req = json!({
            "op": "reset",
            "unix_seconds": unix_seconds,
            "machine_id": random_hex(MACHINE_ID_BYTES)?,
            "hostname": format!("computer-{}", random_hex(HOST_SUFFIX_BYTES)?),
            "entropy_b64": base64::engine::general_purpose::STANDARD.encode(entropy),
        });
        let reply = self.ask_guest(id, uid, &req)?;
        ensure!(reply["ok"] == true, "the guest was not reset: {}", reply["error"].as_str().unwrap_or("no reason given"));
        Ok(())
    }

    pub fn hibernate(&self, id: &ComputerId) -> Result<()> {
        let _lock = self.lock()?;
        let Meta { uid, .. } = self.meta(id)?;
        ensure!(self.is_live(id), "computer {} is not running", id.as_str());
        self.run_as_vm(id, uid, "snapshot")?;
        // The VM's user made these files, so nothing of its must be running
        // while root reads them.
        kill_cgroup(&self.cgroup(id))?;
        let part = self.computer(id).join("snapshot.part");
        remove_if_there(&part)?;
        std::fs::DirBuilder::new().mode(0o700).create(&part).context("make the snapshot's directory")?;
        let key = seal::new_key()?;
        for name in FILES {
            let from = open_from_vm(&self.jail_root(id).join(name), uid)?;
            let to = File::options().write(true).create_new(true).mode(0o600).open(part.join(name)).context("store a snapshot")?;
            seal::seal(&key, &mut BufReader::with_capacity(BUFFER_BYTES, from), &mut BufWriter::with_capacity(BUFFER_BYTES, to))?;
        }
        File::options().write(true).create_new(true).mode(0o600).open(part.join(KEY_FILE))?.write_all(&key)?;
        std::fs::write(part.join(BASE_FILE), self.base_id()?)?;
        remove_if_there(&self.jail(id))?;
        self.discard_snapshot_locked(id)?;
        std::fs::rename(&part, self.snapshot_dir(id)).context("finish the snapshot")
    }

    /// Resumes from the snapshot if there is one for this base image, or boots
    /// cold. Says which, because only a resumed guest needs its resets.
    pub fn wake(&self, id: &ComputerId) -> Result<Wake> {
        let _lock = self.lock()?;
        let dir = self.snapshot_dir(id);
        if !self.has_snapshot(id) || std::fs::read_to_string(dir.join(BASE_FILE))? != self.base_id()? {
            self.discard_snapshot_locked(id)?;
            return Ok(Wake { resumed: false, overlay_fresh: self.boot_locked(id, Purpose::Computer)? });
        }
        self.may_start(id)?;
        let Meta { uid, .. } = self.meta(id)?;
        self.prepare_jail(id, uid, Purpose::Computer)?;
        let key: [u8; KEY_BYTES] = std::fs::read(dir.join(KEY_FILE))?.try_into().map_err(|_| anyhow::anyhow!("a snapshot's key is damaged"))?;
        for name in FILES {
            let to_path = self.jail_root(id).join(name);
            let to = File::options().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&to_path).context("restore a snapshot")?;
            seal::open(&key, &mut BufReader::with_capacity(BUFFER_BYTES, File::open(dir.join(name))?), &mut BufWriter::with_capacity(BUFFER_BYTES, to))?;
            chown(&to_path, Some(uid), Some(uid)).context("give the VM its snapshot")?;
        }
        self.launch(id, uid, false)?;
        // Until the resets are done this VM is a copy of the one that was
        // saved, so it either gets them all or does not stay up.
        if let Err(e) = self.run_as_vm(id, uid, "restore").and_then(|_| self.reset_guest(id, uid)) {
            kill_cgroup(&self.cgroup(id))?;
            return Err(e);
        }
        // The disks have moved on from it the moment the VM runs.
        self.discard_snapshot_locked(id)?;
        Ok(Wake { resumed: true, overlay_fresh: false })
    }
}

fn random_hex(bytes: usize) -> Result<String> {
    let mut raw = vec![0u8; bytes];
    File::open("/dev/urandom").context("open the system's randomness")?.read_exact(&mut raw)?;
    Ok(raw.iter().map(|b| format!("{b:02x}")).collect())
}

#[derive(Serialize, Debug, Clone, Copy)]
pub struct Wake {
    pub resumed: bool,
    pub overlay_fresh: bool,
}

/// A file the VM's user made: never a link, a device or someone else's file.
fn open_from_vm(path: &Path, uid: u32) -> Result<File> {
    let file = File::options().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path).with_context(|| format!("open {}", path.display()))?;
    let meta = file.metadata()?;
    ensure!(meta.is_file() && meta.uid() == uid, "{} is not the VM's own file", path.display());
    Ok(file)
}
