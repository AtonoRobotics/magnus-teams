//! Computerd's own settings, read from a root-owned file. Magnus sets none of them.

use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const ROOT_UID: u32 = 0;
/// Below this are the system's own users.
const MIN_FIRST_UID: u32 = 100_000;
const MAX_COMPUTERS_LIMIT: u32 = 65_536;
/// A month to act on an erasure (spec, the purge window).
pub const MAX_PURGE_DAYS: u32 = 30;
const GROUP_OR_OTHER_WRITE: u32 = 0o022;
pub const FIRECRACKER_NAME: &str = "firecracker";
pub const KERNEL_IMAGE: &str = "vmlinux";
pub const BASE_IMAGE: &str = "rootfs.ext4";

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub state_dir: PathBuf,
    pub socket: PathBuf,
    pub socket_gid: u32,
    pub jailer: PathBuf,
    pub firecracker: PathBuf,
    /// Each computer gets its own uid from `first_uid` up.
    pub first_uid: u32,
    pub max_computers: u32,
    pub max_live: u32,
    /// How long a backup or snapshot may be kept.
    #[serde(default = "default_purge_days")]
    pub purge_days: u32,
    pub vm: VmCaps,
}

fn default_purge_days() -> u32 {
    MAX_PURGE_DAYS
}

/// The same for every computer, and never taken from a request.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct VmCaps {
    pub vcpus: u32,
    pub memory_mib: u32,
    pub cpu_percent: u32,
    pub pids_max: u32,
    pub volume_gib: u32,
    pub overlay_gib: u32,
}

impl Config {
    /// The settings alone, without looking at who owns what. The kill switch
    /// uses this, because it must work on a machine that is otherwise wrong.
    pub fn parse(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let cfg: Config = toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// The settings, once everything root will read is proved to be root's.
    pub fn load(path: &Path) -> Result<Config> {
        let cfg = Config::parse(path)?;
        root_only(path, ROOT_UID)?;
        root_only(&cfg.jailer, ROOT_UID)?;
        root_only(&cfg.firecracker, ROOT_UID)?;
        root_only(&cfg.state_dir, ROOT_UID)?;
        for image in [KERNEL_IMAGE, BASE_IMAGE] {
            let file = cfg.state_dir.join("images").join(image);
            root_only(&file, ROOT_UID)?;
            // A link would be linked into the jail as a link.
            ensure!(std::fs::symlink_metadata(&file)?.is_file(), "{} must be a real file", file.display());
        }
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        for p in [&self.state_dir, &self.socket, &self.jailer, &self.firecracker] {
            ensure!(p.is_absolute(), "{} must be an absolute path", p.display());
        }
        ensure!(self.firecracker.file_name().is_some_and(|n| n == FIRECRACKER_NAME), "firecracker must be a file named {FIRECRACKER_NAME}");
        ensure!(self.first_uid >= MIN_FIRST_UID, "first_uid must be at least {MIN_FIRST_UID}");
        ensure!((1..=MAX_COMPUTERS_LIMIT).contains(&self.max_computers), "max_computers must be 1 to {MAX_COMPUTERS_LIMIT}");
        ensure!(self.first_uid.checked_add(self.max_computers).is_some_and(|end| end < u32::MAX), "the uid range overflows");
        ensure!(self.max_live >= 1, "max_live must be at least 1");
        ensure!((1..=MAX_PURGE_DAYS).contains(&self.purge_days), "purge_days must be 1 to {MAX_PURGE_DAYS}");
        let v = &self.vm;
        ensure!(v.vcpus >= 1 && v.memory_mib >= 1 && v.cpu_percent >= 1 && v.pids_max >= 1, "every vm limit must be at least 1");
        ensure!(v.volume_gib >= 1 && v.overlay_gib >= 1, "every drive must be at least 1 GiB");
        Ok(())
    }
}

/// `path` and every directory above it belong to `owner` and nobody else can
/// write to them, so no other user can swap anything along the way.
pub fn root_only(path: &Path, owner: u32) -> Result<()> {
    let real = path.canonicalize().with_context(|| format!("find {}", path.display()))?;
    for p in real.ancestors() {
        let meta = std::fs::metadata(p).with_context(|| format!("inspect {}", p.display()))?;
        if meta.uid() != owner {
            bail!("{} must be owned by uid {owner}", p.display());
        }
        if meta.mode() & GROUP_OR_OTHER_WRITE != 0 {
            bail!("{} must not be writable by group or others", p.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn config() -> Config {
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
            vcpus = 1
            memory_mib = 256
            cpu_percent = 100
            pids_max = 128
            volume_gib = 1
            overlay_gib = 1
            "#,
        )
        .unwrap()
    }

    #[test]
    fn settings_that_would_collide_with_system_users_are_refused() {
        assert!(config().validate().is_ok());
        let mut c = config();
        c.first_uid = 1000;
        assert!(c.validate().is_err(), "a system uid was accepted");
        let mut c = config();
        c.first_uid = u32::MAX - 1;
        assert!(c.validate().is_err(), "an overflowing range was accepted");
        let mut c = config();
        c.jailer = "jailer".into();
        assert!(c.validate().is_err(), "a relative path was accepted");
    }

    #[test]
    fn an_unknown_setting_is_refused() {
        assert!(toml::from_str::<VmCaps>("vcpus=1\nmemory_mib=1\ncpu_percent=1\npids_max=1\nvolume_gib=1\noverlay_gib=1\nextra=1").is_err());
    }

    #[test]
    fn a_file_another_user_can_change_is_refused() {
        let me = unsafe { libc::geteuid() };
        let dir = tempfile::tempdir_in(std::env::var("CARGO_MANIFEST_DIR").unwrap()).unwrap();
        let file = dir.path().join("f");
        std::fs::write(&file, "x").unwrap();
        // Owned by someone other than `owner`: this user's files are not uid 4242's.
        assert!(root_only(&file, 4242).is_err());
        // Group-writable anywhere along the path is refused, whoever owns it.
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o664)).unwrap();
        let err = format!("{:#}", root_only(&file, me).unwrap_err());
        assert!(err.contains("writable") || err.contains("owned"), "{err}");
    }
}
