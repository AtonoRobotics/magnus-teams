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
/// How the memory and state are sealed (`sparse::FORMAT`); a snapshot
/// without it was sealed whole, before 2026-11-13.
pub const FORMAT_FILE: &str = "format";
const BUFFER_BYTES: usize = 1 << 20;
const RESET_ENTROPY_BYTES: usize = 64;
const MACHINE_ID_BYTES: usize = 16;
const HOST_SUFFIX_BYTES: usize = 8;
const FILES: [&str; 2] = [SNAPSHOT_STATE, SNAPSHOT_MEMORY];
/// A VM with no record of its base image: its snapshot fits none.
const UNKNOWN_BASE: &str = "unknown";

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

    /// The four things a snapshot would otherwise hand to every restore: the
    /// clock, the machine id, the entropy, and the network identity (MAC and
    /// host name).
    fn reset_guest(&self, id: &ComputerId, uid: u32) -> Result<()> {
        let mut entropy = [0u8; RESET_ENTROPY_BYTES];
        File::open("/dev/urandom").context("open the system's randomness")?.read_exact(&mut entropy)?;
        let unix_seconds = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let req = json!({
            "op": "reset",
            "unix_seconds": unix_seconds,
            "machine_id": random_hex(MACHINE_ID_BYTES)?,
            "hostname": format!("computer-{}", random_hex(HOST_SUFFIX_BYTES)?),
            "mac": random_mac()?,
            "entropy_b64": base64::engine::general_purpose::STANDARD.encode(entropy),
        });
        let reply = self.ask_guest(id, uid, &req)?;
        ensure!(reply["ok"] == true, "the guest was not reset: {}", reply["error"].as_str().unwrap_or("no reason given"));
        Ok(())
    }

    pub fn hibernate(&self, id: &ComputerId) -> Result<()> {
        let _lock = self.lock()?;
        let meta = self.meta(id)?;
        let uid = meta.uid;
        ensure!(self.is_live(id), "computer {} is not running", id.as_str());
        let mut took = took::Took::start();
        self.run_as_vm(id, uid, "snapshot")?;
        took.mark("snapshot");
        // The VM's user made these files, so nothing of its must be running
        // while root reads them.
        kill_cgroup(&self.cgroup(id))?;
        self.remove_network(id);
        let part = self.computer(id).join("snapshot.part");
        remove_if_there(&part)?;
        std::fs::DirBuilder::new().mode(0o700).create(&part).context("make the snapshot's directory")?;
        let key = seal::new_key()?;
        let mut bytes = 0;
        for name in FILES {
            let from = open_from_vm(&self.jail_root(id).join(name), uid)?;
            bytes += from.metadata()?.len();
            let to = File::options().write(true).create_new(true).mode(0o600).open(part.join(name)).context("store a snapshot")?;
            crate::sparse::seal(&key, &mut BufReader::with_capacity(BUFFER_BYTES, from), &mut BufWriter::with_capacity(BUFFER_BYTES, to))?;
        }
        File::options().write(true).create_new(true).mode(0o600).open(part.join(KEY_FILE))?.write_all(&key)?;
        std::fs::write(part.join(FORMAT_FILE), crate::sparse::FORMAT)?;
        std::fs::write(part.join(BASE_FILE), snapshot_base(&meta))?;
        remove_if_there(&self.jail(id))?;
        self.discard_snapshot_locked(id)?;
        std::fs::rename(&part, self.snapshot_dir(id)).context("finish the snapshot")?;
        took.mark("seal");
        let steps = took.steps();
        eprintln!("magnus-computerd: computer {} hibernated in {} ms ({}; {} MiB of memory and state)", id.as_str(), steps.total_ms(), steps.words(), bytes / BYTES_PER_MIB);
        Ok(())
    }

    /// Whether a wake may resume from the snapshot: it was made on the base
    /// image installed now, by a VM that booted from it (spec, Rebuild path:
    /// "restore the overlay snapshot if it was made on this base image").
    pub(super) fn resumable(&self, id: &ComputerId) -> Result<bool> {
        if !self.has_snapshot(id) {
            return Ok(false);
        }
        let base = self.base_id()?;
        Ok(std::fs::read_to_string(self.snapshot_dir(id).join(BASE_FILE))? == base && self.meta(id)?.overlay_base.as_deref() == Some(base.as_str()))
    }

    /// Resumes from the snapshot if there is one for this base image, or boots
    /// cold. Says which, because only a resumed guest needs its resets.
    pub fn wake(&self, id: &ComputerId) -> Result<Wake> {
        let _lock = self.lock()?;
        let mut took = took::Took::start();
        if !self.resumable(id)? {
            self.discard_snapshot_locked(id)?;
            let overlay_fresh = self.boot_locked(id, Purpose::Computer, &mut took)?;
            return Ok(self.woke(id, false, overlay_fresh, &took));
        }
        let dir = self.snapshot_dir(id);
        self.may_start(id)?;
        let Meta { uid, .. } = self.meta(id)?;
        self.prepare_jail(id, uid, Purpose::Computer)?;
        let key: [u8; KEY_BYTES] = std::fs::read(dir.join(KEY_FILE))?.try_into().map_err(|_| anyhow::anyhow!("a snapshot's key is damaged"))?;
        for name in FILES {
            let to_path = self.jail_root(id).join(name);
            let mut to = File::options().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&to_path).context("restore a snapshot")?;
            open_stored(&dir, &key, &dir.join(name), &mut to)?;
            chown(&to_path, Some(uid), Some(uid)).context("give the VM its snapshot")?;
        }
        took.mark("open_snapshot");
        self.launch(id, uid, false, true)?;
        took.mark("launch");
        // Until the resets are done this VM is a copy of the one that was
        // saved, so it either gets them all or does not stay up.
        if let Err(e) = self.run_as_vm(id, uid, "restore").and_then(|_| {
            took.mark("restore");
            self.reset_guest(id, uid)
        }) {
            kill_cgroup(&self.cgroup(id))?;
            self.remove_network(id);
            return Err(e);
        }
        took.mark("reset");
        // The disks have moved on from it the moment the VM runs.
        self.discard_snapshot_locked(id)?;
        Ok(self.woke(id, true, false, &took))
    }

    fn woke(&self, id: &ComputerId, resumed: bool, overlay_fresh: bool, took: &took::Took) -> Wake {
        let steps = took.steps();
        let how = if resumed { "resumed from its snapshot" } else { "booted cold" };
        eprintln!("magnus-computerd: computer {} {how} in {} ms ({})", id.as_str(), steps.total_ms(), steps.words());
        Wake { resumed, overlay_fresh, took_ms: steps }
    }
}

/// Opens one stored file of a snapshot into `to`, as it was sealed: sparse
/// when the snapshot says so, else whole.
pub fn open_stored(dir: &Path, key: &[u8; KEY_BYTES], from: &Path, to: &mut File) -> Result<()> {
    let from = BufReader::with_capacity(BUFFER_BYTES, File::open(from)?);
    if std::fs::read_to_string(dir.join(FORMAT_FILE)).is_ok_and(|f| f.trim() == crate::sparse::FORMAT) {
        return crate::sparse::open(key, &mut { from }, to);
    }
    seal::open(key, &mut { from }, &mut BufWriter::with_capacity(BUFFER_BYTES, to))
}

/// What a snapshot is valid on: the base image the VM booted from, which is
/// not always the one installed now. An update installs a new base image
/// under a running VM, which keeps the one it booted from until it stops.
fn snapshot_base(meta: &Meta) -> &str {
    meta.overlay_base.as_deref().unwrap_or(UNKNOWN_BASE)
}

fn random_hex(bytes: usize) -> Result<String> {
    let mut raw = vec![0u8; bytes];
    File::open("/dev/urandom").context("open the system's randomness")?.read_exact(&mut raw)?;
    Ok(raw.iter().map(|b| format!("{b:02x}")).collect())
}

/// Locally administered and not multicast.
fn random_mac() -> Result<String> {
    let mut raw = [0u8; 6];
    File::open("/dev/urandom").context("open the system's randomness")?.read_exact(&mut raw)?;
    raw[0] = (raw[0] | 0x02) & 0xFE;
    Ok(raw.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":"))
}

#[derive(Serialize, Debug, Clone)]
pub struct Wake {
    pub resumed: bool,
    pub overlay_fresh: bool,
    /// Each step's time, for Magnus's log of the whole wake.
    pub took_ms: took::Steps,
}

/// A file the VM's user made: never a link, a device or someone else's file.
fn open_from_vm(path: &Path, uid: u32) -> Result<File> {
    let file = File::options().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path).with_context(|| format!("open {}", path.display()))?;
    let meta = file.metadata()?;
    ensure!(meta.is_file() && meta.uid() == uid, "{} is not the VM's own file", path.display());
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A computer that booted on `booted`, hibernated as `hibernate` records
    /// it, on a machine whose base image is now `installed`.
    fn hibernated(state: &Path, booted: &str, installed: &str) -> (Computers, ComputerId) {
        let mut cfg = super::super::tests::config();
        cfg.state_dir = state.into();
        let mut c = Computers::new(cfg);
        c.cgroups = state.join("cgroups");
        let id = ComputerId::parse("abc").unwrap();
        std::fs::create_dir_all(c.snapshot_dir(&id)).unwrap();
        std::fs::create_dir_all(state.join("images")).unwrap();
        std::fs::write(state.join("images").join(BASE_ID), installed).unwrap();
        let meta = Meta { uid: 200000, overlay_base: Some(booted.into()) };
        c.write_meta(&id, &meta).unwrap();
        std::fs::write(c.snapshot_dir(&id).join(BASE_FILE), snapshot_base(&meta)).unwrap();
        (c, id)
    }

    #[test]
    fn a_vm_that_booted_on_the_old_base_image_wakes_on_the_new_one() {
        let state = tempfile::tempdir().unwrap();
        // Booted on A; B was installed under it; then it hibernated.
        let (c, id) = hibernated(state.path(), "base-a", "base-b");
        assert!(!c.resumable(&id).unwrap(), "a snapshot of a VM on the old base image was resumed on the new one");
        // As an earlier computerd labelled it: with the base installed when
        // it hibernated, not the one the VM ran.
        std::fs::write(c.snapshot_dir(&id).join(BASE_FILE), "base-b").unwrap();
        assert!(!c.resumable(&id).unwrap(), "a mislabelled snapshot of a VM on the old base image was resumed");
    }

    #[test]
    fn a_vm_that_booted_on_the_installed_base_image_resumes() {
        let state = tempfile::tempdir().unwrap();
        let (c, id) = hibernated(state.path(), "base-b", "base-b");
        assert!(c.resumable(&id).unwrap());
        c.discard_snapshot(&id).unwrap();
        assert!(!c.resumable(&id).unwrap(), "a discarded snapshot was resumed");
    }
}
