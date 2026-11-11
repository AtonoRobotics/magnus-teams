//! The computers themselves. Every path comes from the state directory and a
//! validated id. What is running is read from the kernel's cgroups, never
//! from a file inside a jail, which the VM's own user could change.

use crate::config::{Config, BASE_IMAGE, FIRECRACKER_NAME, KERNEL_IMAGE};
use crate::id::ComputerId;
use crate::relay::VSOCK_IN_JAIL;
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::File;
use std::io::Write as _;
use std::os::unix::fs::{chown, DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub mod backup;
mod guest;
pub mod network;
mod sealed;
mod snapshot;
mod sparse;
pub use snapshot::Wake;

pub const CGROUP_ROOT: &str = "/sys/fs/cgroup/magnus-computers";
const PARENT_CGROUP: &str = "magnus-computers";
const MKFS: &str = "/usr/sbin/mkfs.ext4";
/// Firecracker's own memory, on top of the guest's.
const VMM_OVERHEAD_MIB: u64 = 256;
/// The host's cache of this VM's writes is charged to the VM. Above this mark
/// the writer is slowed while it drains; at the hard limit the VM is killed.
const MEMORY_HIGH_BELOW_MAX_MIB: u64 = 128;
const CPU_PERIOD_US: u64 = 100_000;
const MAX_OPEN_FILES: u32 = 4096;
const BYTES_PER_MIB: u64 = 1 << 20;
const BYTES_PER_GIB: u64 = 1 << 30;
const START_WAIT: Duration = Duration::from_secs(10);
const START_SETTLE: Duration = Duration::from_millis(300);
const KILL_WAIT: Duration = Duration::from_secs(10);
/// A stuck start must not hold up a kill for long.
const LOCK_WAIT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const VM_CONFIG: &str = "vm.json";
const BASE_ID: &str = "base.id";
const VOLUME: &str = "volume.ext4";
/// A guest's sync reaches the host's disk. Firecracker otherwise drops it, and
/// the dirty pages count against the VM's memory limit until the VM is killed.
const WRITABLE_DRIVE_CACHE: &str = "Writeback";
const OVERLAY: &str = "overlay.ext4";
/// The guest's existing name for its writable data drive.
const VOLUME_LABEL: &str = "workspace";
const OVERLAY_LABEL: &str = "overlay";
const GUEST_CID: u32 = 3;
const BOOT_ARGS: &str = "console=ttyS0 reboot=k panic=1 pci=off quiet loglevel=3 init=/sbin/magnus-guest";
const BROWSER_BOOT_ARG: &str = "magnus.role=browser";

#[derive(Serialize, Deserialize)]
struct Meta {
    uid: u32,
    /// The base image its overlay was made on: valid on no other.
    #[serde(default)]
    overlay_base: Option<String>,
}

#[derive(Serialize, Debug, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Stopped,
    Running,
    Paused,
    Hibernated,
}

/// A helper VM reaches a stopped computer's files. It has the volume and
/// nothing else of the computer, and will never get a network.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Computer,
    Helper,
    /// For reading a hibernated computer: leaves its volume as the memory
    /// snapshot expects it.
    ReadHelper,
    /// One identity's browser: its volume holds only the profile, and the
    /// guest serves page commands and no shell.
    Browser,
}

pub struct Computers {
    cfg: Config,
    cgroups: PathBuf,
    /// One volume sealed or opened at a time.
    sealing: std::sync::Mutex<()>,
    /// One backup at a time, and without the lock that a kill needs.
    backing_up: std::sync::Mutex<()>,
}

impl Computers {
    pub fn new(cfg: Config) -> Computers {
        Computers { cfg, cgroups: PathBuf::from(CGROUP_ROOT), sealing: std::sync::Mutex::new(()), backing_up: std::sync::Mutex::new(()) }
    }

    fn state(&self, rest: &str) -> PathBuf {
        self.cfg.state_dir.join(rest)
    }

    fn computer(&self, id: &ComputerId) -> PathBuf {
        self.state("computers").join(id.as_str())
    }

    /// Where the jailer builds this computer's chroot.
    fn jail(&self, id: &ComputerId) -> PathBuf {
        self.state("jail").join(FIRECRACKER_NAME).join(id.as_str())
    }

    pub fn jail_root(&self, id: &ComputerId) -> PathBuf {
        self.jail(id).join("root")
    }

    /// Where a relay for this computer locks itself in, and as whom.
    pub fn relay_target(&self, id: &ComputerId) -> Result<(PathBuf, u32)> {
        let Meta { uid, .. } = self.meta(id)?;
        ensure!(self.is_live(id), "computer {} is not running", id.as_str());
        Ok((self.jail_root(id), uid))
    }

    fn cgroup(&self, id: &ComputerId) -> PathBuf {
        self.cgroups.join(id.as_str())
    }

    fn halted_marker(&self) -> PathBuf {
        self.state("halted")
    }

    /// One change at a time, across this service and the owner's kill switch.
    fn lock(&self) -> Result<File> {
        let file = File::options().create(true).write(true).truncate(false).mode(0o600).open(self.state("lock")).context("open the lock")?;
        file.lock().context("take the lock")?;
        Ok(file)
    }

    /// The lock if it can be had soon. A kill goes ahead without it.
    fn lock_for_kill(&self) -> Option<File> {
        let file = File::options().create(true).write(true).truncate(false).mode(0o600).open(self.state("lock")).ok()?;
        let started = Instant::now();
        while started.elapsed() < LOCK_WAIT {
            if file.try_lock().is_ok() {
                return Some(file);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        None
    }

    fn meta(&self, id: &ComputerId) -> Result<Meta> {
        let text = std::fs::read_to_string(self.computer(id).join("meta.json")).with_context(|| format!("no computer {}", id.as_str()))?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn list(&self) -> Result<Vec<ComputerId>> {
        let mut ids = vec![];
        for e in std::fs::read_dir(self.state("computers")).context("list computers")? {
            if let Some(id) = e?.file_name().to_str().and_then(|n| ComputerId::parse(n).ok()) {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    fn free_uid(&self) -> Result<u32> {
        let used = self.list()?.iter().filter_map(|id| self.meta(id).ok()).map(|m| m.uid).collect::<Vec<_>>();
        (self.cfg.first_uid..self.cfg.first_uid + self.cfg.max_computers).find(|u| !used.contains(u)).context("this machine has no room for another computer")
    }

    pub fn create(&self, id: &ComputerId) -> Result<u32> {
        let _lock = self.lock()?;
        let dir = self.computer(id);
        ensure!(!dir.exists(), "computer {} already exists", id.as_str());
        let uid = self.free_uid()?;
        std::fs::DirBuilder::new().mode(0o700).create(&dir).context("make the computer's directory")?;
        let made = std::fs::write(dir.join("meta.json"), serde_json::to_vec(&Meta { uid, overlay_base: None })?)
            .map_err(anyhow::Error::from)
            .and_then(|_| make_drive(&dir.join(VOLUME), self.cfg.vm.volume_gib, uid, VOLUME_LABEL));
        if let Err(e) = made {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
        Ok(uid)
    }

    /// Which computer runs as `uid`, for the egress proxy to name a connection.
    /// How many computers run now and exist, against what this machine
    /// allows: the admin's "computers and capacity" (spec rev 141).
    pub fn capacity(&self) -> Result<serde_json::Value> {
        Ok(serde_json::json!({ "live": self.live_count()?, "max_live": self.cfg.max_live, "computers": self.list()?.len(), "max_computers": self.cfg.max_computers }))
    }

    pub fn whose_uid(&self, uid: u32) -> Result<Option<ComputerId>> {
        for id in self.list()? {
            if self.meta(&id)?.uid == uid {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    pub fn status(&self, id: &ComputerId) -> Result<State> {
        self.meta(id)?;
        if !self.is_live(id) {
            return Ok(if self.has_snapshot(id) { State::Hibernated } else { State::Stopped });
        }
        let frozen = std::fs::read_to_string(self.cgroup(id).join("cgroup.freeze")).unwrap_or_default();
        Ok(if frozen.trim() == "1" { State::Paused } else { State::Running })
    }

    fn is_live(&self, id: &ComputerId) -> bool {
        has_processes(&self.cgroup(id))
    }

    fn live_count(&self) -> Result<u32> {
        let Ok(dir) = std::fs::read_dir(&self.cgroups) else { return Ok(0) };
        let mut n = 0;
        for e in dir {
            let e = e?;
            if e.file_type()?.is_dir() && has_processes(&e.path()) {
                n += 1;
            }
        }
        Ok(n)
    }

    pub fn start(&self, id: &ComputerId, purpose: Purpose) -> Result<bool> {
        let _lock = self.lock()?;
        self.boot_locked(id, purpose)
    }

    fn may_start(&self, id: &ComputerId) -> Result<()> {
        ensure!(!self.halted_marker().exists(), "the owner has halted every computer");
        ensure!(!self.is_live(id), "computer {} is already running", id.as_str());
        ensure!(self.live_count()? < self.cfg.max_live, "this machine is running as many computers as it may");
        Ok(())
    }

    pub fn base_id(&self) -> Result<String> {
        Ok(std::fs::read_to_string(self.state("images").join(BASE_ID)).context("read the base image's id")?.trim().to_string())
    }

    fn write_meta(&self, id: &ComputerId, meta: &Meta) -> Result<()> {
        std::fs::write(self.computer(id).join("meta.json"), serde_json::to_vec(meta)?).context("record the computer")
    }

    /// A cold boot. True when the overlay is new, so its apps must be replayed.
    fn boot_locked(&self, id: &ComputerId, purpose: Purpose) -> Result<bool> {
        self.may_start(id)?;
        let mut meta = self.meta(id)?;
        let dir = self.computer(id);
        let mut fresh = false;
        if purpose == Purpose::Helper {
            // An edit makes the memory snapshot wrong about the disk.
            self.discard_snapshot_locked(id)?;
        }
        if purpose == Purpose::Computer {
            let base = self.base_id()?;
            if meta.overlay_base.as_deref() != Some(&base) {
                remove_file_if_there(&dir.join(OVERLAY))?;
                self.discard_snapshot_locked(id)?;
            }
            if !dir.join(OVERLAY).exists() {
                make_drive(&dir.join(OVERLAY), self.cfg.vm.overlay_gib, meta.uid, OVERLAY_LABEL)?;
                fresh = true;
            }
            meta.overlay_base = Some(base);
            self.write_meta(id, &meta)?;
        }
        self.prepare_jail(id, meta.uid, purpose)?;
        self.launch(id, meta.uid, true, matches!(purpose, Purpose::Computer | Purpose::Browser))?;
        Ok(fresh)
    }

    /// An empty jail holding this computer's images and drives.
    fn prepare_jail(&self, id: &ComputerId, uid: u32, purpose: Purpose) -> Result<()> {
        let dir = self.computer(id);
        let _ = std::fs::remove_dir(self.cgroup(id));
        remove_if_there(&self.jail(id))?;
        let root = self.jail_root(id);
        std::fs::create_dir_all(&root).context("make the jail")?;
        let images = self.state("images");
        // Hard links: one inode each, so the drives outlive the jail and a
        // base image is shared without being writable by anyone here.
        let mut links = vec![(images.join(KERNEL_IMAGE), KERNEL_IMAGE), (images.join(BASE_IMAGE), BASE_IMAGE), (dir.join(VOLUME), VOLUME)];
        if purpose == Purpose::Computer {
            links.push((dir.join(OVERLAY), OVERLAY));
        }
        for (from, name) in links {
            std::fs::hard_link(&from, root.join(name)).with_context(|| format!("link {name} into the jail"))?;
        }
        let config = root.join(VM_CONFIG);
        std::fs::write(&config, serde_json::to_vec(&vm_config(&self.cfg, purpose))?).context("write the VM's config")?;
        // Readable by the VM's own user whatever the umask.
        chown(&config, Some(uid), Some(uid)).context("give the VM its config")?;
        Ok(())
    }

    /// Starts the jailer. `configured` boots from the VM's config; otherwise
    /// the VM waits on its API for a snapshot to load.
    fn launch(&self, id: &ComputerId, uid: u32, configured: bool, networked: bool) -> Result<()> {
        let netns = if networked {
            self.prepare_network(id, uid)?;
            Some(self.netns_path(id))
        } else {
            None
        };
        let out = Command::new(&self.cfg.jailer).env_clear().args(jailer_args(&self.cfg, id, uid, configured, netns.as_deref())).output().context("run the jailer")?;
        if !out.status.success() {
            bail!("the jailer failed: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        let started = Instant::now();
        while !self.is_live(id) {
            ensure!(started.elapsed() < START_WAIT, "computer {} did not start", id.as_str());
            std::thread::sleep(POLL_INTERVAL);
        }
        // A VM that cannot run exits at once, and nothing else would say so:
        // a daemonized jailer has already reported success.
        std::thread::sleep(START_SETTLE);
        ensure!(self.is_live(id), "computer {} exited as it started", id.as_str());
        if networked {
            self.start_forwarder(id, uid)?;
        }
        Ok(())
    }

    fn freeze(&self, id: &ComputerId, frozen: bool) -> Result<()> {
        let _lock = self.lock()?;
        ensure!(self.is_live(id), "computer {} is not running", id.as_str());
        std::fs::write(self.cgroup(id).join("cgroup.freeze"), if frozen { "1" } else { "0" }).context("change the computer's freeze state")
    }

    pub fn pause(&self, id: &ComputerId) -> Result<()> {
        self.freeze(id, true)
    }

    pub fn resume(&self, id: &ComputerId) -> Result<()> {
        self.freeze(id, false)
    }

    pub fn stop(&self, id: &ComputerId) -> Result<()> {
        let _lock = self.lock_for_kill();
        kill_cgroup(&self.cgroup(id))?;
        remove_if_there(&self.jail(id))
    }

    /// Stops the computer and drops what can be rebuilt. The volume is the
    /// person's work and goes only when they ask.
    pub fn destroy(&self, id: &ComputerId, delete_volume: bool) -> Result<()> {
        self.meta(id)?;
        self.stop(id)?;
        let _lock = self.lock()?;
        self.remove_network(id);
        let dir = self.computer(id);
        if delete_volume {
            {
                // Its backups hold the same files, so they go with them.
                let _one_at_a_time = self.backing_up.lock().unwrap_or_else(|e| e.into_inner());
                remove_if_there(&self.state("backups").join(id.as_str()))?;
            }
            std::fs::remove_dir_all(&dir).context("delete the computer")
        } else {
            self.discard_snapshot_locked(id)?;
            remove_file_if_there(&dir.join(OVERLAY))
        }
    }

    /// A reset that erases the person's files (spec rev 142): the same
    /// computer, its id and its user, rebuilt from the base image on its next
    /// start, with a new, empty volume. The old volume and its backups go
    /// now, as a deleted computer's do.
    pub fn erase_volume(&self, id: &ComputerId) -> Result<()> {
        let meta = self.meta(id)?;
        self.destroy(id, false)?;
        let _lock = self.lock()?;
        {
            let _one_at_a_time = self.backing_up.lock().unwrap_or_else(|e| e.into_inner());
            remove_if_there(&self.state("backups").join(id.as_str()))?;
        }
        let volume = self.computer(id).join(VOLUME);
        remove_file_if_there(&volume)?;
        make_drive(&volume, self.cfg.vm.volume_gib, meta.uid, VOLUME_LABEL)
    }

    /// Blocks new starts, then stops every computer.
    pub fn kill_all(&self) -> Result<usize> {
        std::fs::write(self.halted_marker(), "").context("mark the computers halted")?;
        let _lock = self.lock_for_kill();
        let killed = kill_every_computer(&self.cgroups)?;
        let jails = self.state("jail").join(FIRECRACKER_NAME);
        if jails.exists() {
            remove_if_there(&jails)?;
        }
        Ok(killed)
    }

    /// Allows starts again. Nothing is started.
    pub fn release(&self) -> Result<()> {
        match std::fs::remove_file(self.halted_marker()) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }
}

/// Kills every VM on the machine. Needs nothing but the kernel's cgroups.
pub fn kill_every_computer(cgroups: &Path) -> Result<usize> {
    let Ok(dir) = std::fs::read_dir(cgroups) else { return Ok(0) };
    let mut killed = 0;
    for e in dir {
        let e = e?;
        if e.file_type()?.is_dir() {
            if has_processes(&e.path()) {
                killed += 1;
            }
            kill_cgroup(&e.path())?;
        }
    }
    Ok(killed)
}

fn has_processes(cgroup: &Path) -> bool {
    std::fs::read_to_string(cgroup.join("cgroup.procs")).is_ok_and(|p| !p.trim().is_empty())
}

fn kill_cgroup(cgroup: &Path) -> Result<()> {
    if !cgroup.exists() {
        return Ok(());
    }
    std::fs::write(cgroup.join("cgroup.kill"), "1").context("kill the computer")?;
    let started = Instant::now();
    loop {
        // The directory goes only when the last process has.
        match std::fs::remove_dir(cgroup) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => ensure!(started.elapsed() < KILL_WAIT, "a computer's processes did not exit: {e}"),
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn remove_file_if_there(file: &Path) -> Result<()> {
    match std::fs::remove_file(file) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).with_context(|| format!("remove {}", file.display())),
        _ => Ok(()),
    }
}

fn remove_if_there(dir: &Path) -> Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).with_context(|| format!("remove {}", dir.display())),
        _ => Ok(()),
    }
}

/// A new, empty, sparse filesystem. Root only ever runs mkfs on a file it has
/// just made: a drive a guest has written to is never parsed on the host.
fn make_drive(path: &Path, gib: u32, uid: u32, label: &str) -> Result<()> {
    let part = path.with_extension("part");
    let _ = std::fs::remove_file(&part);
    let file = File::options().write(true).create_new(true).mode(0o600).open(&part).context("make a drive")?;
    file.set_len(u64::from(gib) * BYTES_PER_GIB).context("size a drive")?;
    drop(file);
    let out = Command::new(MKFS).env_clear().args(["-q", "-F", "-L", label]).arg(&part).output().context("run mkfs")?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&part);
        bail!("mkfs failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    chown(&part, Some(uid), Some(uid)).context("give the drive to its computer")?;
    std::fs::rename(&part, path).context("finish a drive")
}

/// Paths are as Firecracker sees them, inside the chroot.
fn vm_config(cfg: &Config, purpose: Purpose) -> serde_json::Value {
    let mut drives = vec![
        json!({ "drive_id": "rootfs", "path_on_host": BASE_IMAGE, "is_root_device": true, "is_read_only": true }),
        json!({ "drive_id": "volume", "path_on_host": VOLUME, "is_root_device": false, "is_read_only": purpose == Purpose::ReadHelper, "cache_type": WRITABLE_DRIVE_CACHE }),
    ];
    if purpose == Purpose::Computer {
        drives.push(json!({ "drive_id": "overlay", "path_on_host": OVERLAY, "is_root_device": false, "is_read_only": false, "cache_type": WRITABLE_DRIVE_CACHE }));
    }
    let boot_args = if purpose == Purpose::Browser { format!("{BOOT_ARGS} {BROWSER_BOOT_ARG}") } else { BOOT_ARGS.to_string() };
    let mut v = json!({
        "boot-source": { "kernel_image_path": KERNEL_IMAGE, "boot_args": boot_args },
        "drives": drives,
        "machine-config": { "vcpu_count": cfg.vm.vcpus, "mem_size_mib": cfg.vm.memory_mib },
        "vsock": { "guest_cid": GUEST_CID, "uds_path": VSOCK_IN_JAIL.trim_start_matches('/') },
    });
    if matches!(purpose, Purpose::Computer | Purpose::Browser) {
        v["network-interfaces"] = json!([{ "iface_id": "eth0", "host_dev_name": network::TAP, "guest_mac": network::GUEST_MAC }]);
    }
    v
}

fn jailer_args(cfg: &Config, id: &ComputerId, uid: u32, configured: bool, netns: Option<&Path>) -> Vec<String> {
    let v = &cfg.vm;
    let memory_max = (u64::from(v.memory_mib) + VMM_OVERHEAD_MIB) * BYTES_PER_MIB;
    let cpu_quota = CPU_PERIOD_US * u64::from(v.cpu_percent) / 100;
    let largest_drive = u64::from(v.volume_gib.max(v.overlay_gib)) * BYTES_PER_GIB;
    let mut args: Vec<String> = vec![];
    let mut pair = |flag: &str, value: String| {
        args.push(flag.to_string());
        args.push(value);
    };
    pair("--id", id.as_str().to_string());
    pair("--exec-file", cfg.firecracker.display().to_string());
    pair("--uid", uid.to_string());
    pair("--gid", uid.to_string());
    pair("--chroot-base-dir", cfg.state_dir.join("jail").display().to_string());
    pair("--cgroup-version", "2".to_string());
    pair("--parent-cgroup", PARENT_CGROUP.to_string());
    pair("--cgroup", format!("memory.high={}", memory_max - MEMORY_HIGH_BELOW_MAX_MIB * BYTES_PER_MIB));
    pair("--cgroup", format!("memory.max={memory_max}"));
    pair("--cgroup", "memory.swap.max=0".to_string());
    pair("--cgroup", format!("cpu.max={cpu_quota} {CPU_PERIOD_US}"));
    pair("--cgroup", format!("pids.max={}", v.pids_max));
    pair("--resource-limit", format!("fsize={largest_drive}"));
    pair("--resource-limit", format!("no-file={MAX_OPEN_FILES}"));
    if let Some(ns) = netns {
        args.extend(["--netns".to_string(), ns.display().to_string()]);
    }
    args.extend(["--new-pid-ns", "--daemonize"].map(String::from));
    if configured {
        args.extend(["--", "--config-file", VM_CONFIG].map(String::from));
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn config() -> Config {
        toml::from_str(
            r#"
            state_dir = "/var/lib/c"
            socket = "/run/c.sock"
            socket_gid = 984
            jailer = "/usr/local/bin/jailer"
            firecracker = "/usr/local/bin/firecracker"
            first_uid = 200000
            max_computers = 8
            max_live = 2
            [vm]
            vcpus = 2
            memory_mib = 1024
            cpu_percent = 150
            pids_max = 128
            volume_gib = 20
            overlay_gib = 10
            "#,
        )
        .unwrap()
    }

    fn after<'a>(args: &'a [String], flag: &str) -> Vec<&'a str> {
        args.windows(2).filter(|w| w[0] == flag).map(|w| w[1].as_str()).collect()
    }

    #[test]
    fn a_vm_runs_as_its_own_user_with_limits_from_the_config() {
        let id = ComputerId::parse("abc").unwrap();
        let args = jailer_args(&config(), &id, 200003, true, None);
        assert_eq!(after(&args, "--uid"), ["200003"]);
        assert_eq!(after(&args, "--gid"), ["200003"]);
        assert_eq!(after(&args, "--id"), ["abc"]);
        let cgroups = after(&args, "--cgroup");
        assert!(cgroups.contains(&"memory.max=1342177280"), "{cgroups:?}");
        assert!(cgroups.contains(&"cpu.max=150000 100000"), "{cgroups:?}");
        assert!(cgroups.contains(&"pids.max=128"), "{cgroups:?}");
        assert!(after(&args, "--resource-limit").contains(&"fsize=21474836480"));
    }

    #[test]
    fn deleting_a_computer_deletes_its_backups_and_a_reset_keeps_them() {
        use std::os::unix::fs::MetadataExt;
        let state = tempfile::tempdir().unwrap();
        let mut cfg = config();
        cfg.state_dir = state.path().into();
        let mut c = Computers::new(cfg);
        c.cgroups = state.path().join("no-cgroups");
        let uid = std::fs::metadata(state.path()).unwrap().uid();
        for name in ["abc", "abd"] {
            let id = ComputerId::parse(name).unwrap();
            std::fs::create_dir_all(c.computer(&id)).unwrap();
            std::fs::write(c.computer(&id).join("meta.json"), json!({ "uid": uid }).to_string()).unwrap();
            std::fs::write(c.computer(&id).join(OVERLAY), b"system").unwrap();
            std::fs::create_dir_all(state.path().join("backups").join(name)).unwrap();
            std::fs::write(state.path().join("backups").join(name).join("key"), b"k").unwrap();
        }
        c.destroy(&ComputerId::parse("abd").unwrap(), false).unwrap();
        assert!(state.path().join("backups/abd/key").exists(), "a reset that keeps the files lost their backups");
        assert!(!state.path().join("computers/abd").join(OVERLAY).exists(), "a reset kept the old system");
        c.destroy(&ComputerId::parse("abc").unwrap(), true).unwrap();
        assert!(!state.path().join("backups/abc").exists(), "a deleted computer's backups were kept");
        assert!(!state.path().join("computers/abc").exists());
        assert!(state.path().join("backups/abd/key").exists(), "another computer's backups went");
    }

    #[test]
    fn erasing_the_volume_keeps_the_computer_and_empties_its_files() {
        use std::os::unix::fs::MetadataExt;
        let state = tempfile::tempdir().unwrap();
        let mut cfg = config();
        cfg.state_dir = state.path().into();
        cfg.vm.volume_gib = 1;
        let mut c = Computers::new(cfg);
        c.cgroups = state.path().join("no-cgroups");
        let uid = std::fs::metadata(state.path()).unwrap().uid();
        let id = ComputerId::parse("abc").unwrap();
        let dir = c.computer(&id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("meta.json"), json!({ "uid": uid }).to_string()).unwrap();
        std::fs::write(dir.join(OVERLAY), b"system").unwrap();
        std::fs::write(dir.join(VOLUME), b"the person's files").unwrap();
        std::fs::create_dir_all(state.path().join("backups/abc")).unwrap();
        std::fs::write(state.path().join("backups/abc/key"), b"k").unwrap();
        c.erase_volume(&id).unwrap();
        assert!(dir.join("meta.json").exists(), "the computer itself went");
        assert!(!dir.join(OVERLAY).exists(), "the old system was kept");
        assert!(!state.path().join("backups/abc").exists(), "the erased files' backups were kept");
        let fresh = std::fs::read(dir.join(VOLUME)).unwrap();
        assert!(!fresh.windows(18).any(|w| w == b"the person's files"), "the old files are still on the volume");
        assert_eq!(std::fs::metadata(dir.join(VOLUME)).unwrap().len(), BYTES_PER_GIB, "the new volume is not the configured size");
    }

    #[test]
    fn every_path_comes_from_the_state_directory_and_the_id() {
        let c = Computers::new(config());
        let id = ComputerId::parse("abc").unwrap();
        assert_eq!(c.computer(&id), Path::new("/var/lib/c/computers/abc"));
        assert_eq!(c.jail_root(&id), Path::new("/var/lib/c/jail/firecracker/abc/root"));
    }

    #[test]
    fn the_vm_has_no_network_and_only_paths_inside_its_chroot() {
        let helper = vm_config(&config(), Purpose::Helper);
        assert!(helper.get("network-interfaces").is_none());
        assert_eq!(helper["drives"].as_array().unwrap().len(), 2, "a helper VM got more than the volume");
        assert!(helper.get("network-interfaces").is_none(), "a helper VM got a network");
        let v = vm_config(&config(), Purpose::Computer);
        assert_eq!(v["network-interfaces"][0]["host_dev_name"], network::TAP);
        let text = v.to_string();
        assert!(!text.contains("/var/lib"), "{text}");
        assert_eq!(v["drives"][0]["is_read_only"], true);
        assert_eq!(v["drives"][1]["cache_type"], "Writeback", "a guest's sync would stop at the host's page cache");
    }
}
