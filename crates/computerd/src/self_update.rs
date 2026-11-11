//! `magnus-computerd self-update`: root installs a newer computerd, and the
//! images it starts computers from, out of a signed appliance release, and
//! only a signed one (`decisions/2026-11-11-computerd-version-and-update.md`).
//!
//! Magnus updates itself as the user it runs as, and that user owns the
//! release directory, the package and the images the package links to. Root
//! must never trust a file that user can write (see the installer). So
//! nothing there is taken on its word: the package is read once into
//! memory, its signature checked with the release key root keeps
//! (`/etc/magnus/release.pub`), exactly as Magnus checks it, and only what
//! was signed is unpacked, into a directory of root's. The images are not
//! in the package: the package names them by sha256 (`images.sha256`), and
//! each is copied into root's directory and hashed there, so the bytes that
//! are checked are the bytes that are installed. Then the release's own
//! installer runs from root's copy. A release older than, or the same as,
//! the one installed is never installed.

use crate::version::{installed, VERSION_FILE};
use anyhow::{bail, ensure, Context, Result};
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// Where the release directory is and who Magnus runs as; written by the
/// installer when it is given the release directory.
pub const SETTINGS: &str = "/etc/magnus/computerd-update.toml";
/// The release key root checks against: a copy of root's own, never the
/// one beside Magnus.
pub const RELEASE_KEY: &str = "/etc/magnus/release.pub";
/// Where the installer puts the program and its VERSION.
pub const PROGRAM_DIR: &str = "/usr/local/lib/magnus";
/// The project's release key, as built in; the installer copies it to
/// `RELEASE_KEY` the first time.
pub const BUILT_IN_KEY: &str = include_str!("../../../release/update.pub");

const MAX_MANIFEST_BYTES: u64 = 1 << 20;
const MAX_PACKAGE_BYTES: u64 = 1 << 30;
const MAX_MEMBER_BYTES: u64 = 256 << 20;
const HASH_BUFFER_BYTES: usize = 1 << 20;
const STAGING_MODE: u32 = 0o700;
const MEMBER_MODE: u32 = 0o600;
const PROGRAM_MODE: u32 = 0o700;
/// What the installer and `cp` see; nothing from the caller's environment.
const SAFE_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";

const PROGRAM: &str = "magnus-computerd";
const INSTALLER: &str = "install-computerd.sh";
const UNIT: &str = "magnus-computerd.service";
const HASHES: &str = "images.sha256";
const KERNEL: &str = "vmlinux";
const ROOTFS: &str = "rootfs.ext4";
const MEMBERS: [&str; 5] = [VERSION_FILE, PROGRAM, INSTALLER, UNIT, HASHES];
const BASE_ID: &str = "base.id";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// The appliance release directory: `latest.json` and the packages.
    pub releases: PathBuf,
    /// The directory a release's image links are relative to: Magnus's
    /// install directory.
    pub links_from: PathBuf,
    /// The user Magnus runs as, for the installer.
    pub user: String,
}

impl Settings {
    pub fn load(path: &Path, owner: u32) -> Result<Settings> {
        crate::config::root_only(path, owner)?;
        let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parse {}", path.display()))
    }
}

/// Everything the update reads or writes.
pub struct Paths {
    pub releases: PathBuf,
    pub links_from: PathBuf,
    pub key: PathBuf,
    pub program_dir: PathBuf,
    /// The installed images and their `base.id`.
    pub images_dir: PathBuf,
    /// Root's own directory the release is unpacked into.
    pub staging: PathBuf,
    /// The owner every trusted file must have; `None` only in tests.
    pub owner: Option<u32>,
}

/// A release unpacked and checked, ready for its installer.
#[derive(Debug)]
pub struct Staged {
    pub version: String,
    pub installer: PathBuf,
    pub program: PathBuf,
    pub kernel: PathBuf,
    pub rootfs: PathBuf,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    UpToDate { installed: String, latest: String },
    Installed { from: String, to: String },
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
    file: String,
    signature: String,
}

/// The same check Magnus makes (`crates/magnus/src/update.rs`, `verify`):
/// the signature verifies with the release key and binds `version`.
pub fn verify(data: &[u8], signature_b64: &str, public_key_b64: &str, version: &str) -> Result<()> {
    let text = |b: &str| -> Result<String> { Ok(String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b.trim())?)?) };
    let pk = minisign_verify::PublicKey::decode(&text(public_key_b64).context("the release key")?).map_err(|e| anyhow::anyhow!("the release key: {e}"))?;
    let sig = minisign_verify::Signature::decode(&text(signature_b64).context("the signature")?).map_err(|e| anyhow::anyhow!("the signature: {e}"))?;
    pk.verify(data, &sig, false).map_err(|_| anyhow::anyhow!("the signature does not verify with the release key"))?;
    let signed = sig.trusted_comment().split('\t').find_map(|f| f.strip_prefix("version:")).context("the signature names no version")?;
    ensure!(signed == version, "the package was signed as {signed}, not {version}");
    Ok(())
}

fn newer(a: &str, than: &str) -> bool {
    match (semver::Version::parse(a), semver::Version::parse(than)) {
        (Ok(a), Ok(b)) => a > b,
        _ => false,
    }
}

fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?.take(limit + 1).read_to_end(&mut out)?;
    ensure!(out.len() as u64 <= limit, "{} is too large", path.display());
    Ok(out)
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; HASH_BUFFER_BYTES];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// Install the latest release when it is newer than what is installed.
/// `install` runs the staged installer; nothing reaches it unchecked.
pub fn run(p: &Paths, install: impl FnOnce(&Staged) -> Result<()>) -> Result<Outcome> {
    if let Some(owner) = p.owner {
        crate::config::root_only(&p.key, owner)?;
        crate::config::root_only(&p.program_dir, owner)?;
    }
    let key = std::fs::read_to_string(&p.key).with_context(|| format!("read {}", p.key.display()))?;
    let m: Manifest = serde_json::from_slice(&read_limited(&p.releases.join("latest.json"), MAX_MANIFEST_BYTES)?).context("parse latest.json")?;
    let current = installed(Some(&p.program_dir));
    if !newer(&m.version, &current) {
        return Ok(Outcome::UpToDate { installed: current, latest: m.version });
    }
    ensure!(!m.file.is_empty() && !m.file.contains('/') && !m.file.contains(".."), "not a release file name: {}", m.file);
    let package = read_limited(&p.releases.join(&m.file), MAX_PACKAGE_BYTES)?;
    verify(&package, &m.signature, &key, &m.version)?;
    let result = stage(p, &package, &m.version).and_then(|staged| install(&staged));
    let _ = std::fs::remove_dir_all(&p.staging);
    result?;
    Ok(Outcome::Installed { from: current, to: m.version })
}

/// Unpack what was signed into root's directory and bring the images in.
fn stage(p: &Paths, package: &[u8], version: &str) -> Result<Staged> {
    if p.staging.exists() {
        std::fs::remove_dir_all(&p.staging)?;
    }
    if let Some(parent) = p.staging.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::DirBuilder::new().mode(STAGING_MODE).create(&p.staging).with_context(|| format!("make {}", p.staging.display()))?;
    let mut links: Vec<(String, PathBuf)> = Vec::new();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(package));
    for entry in archive.entries().context("read the package")? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let name = path.strip_prefix(".").unwrap_or(&path).to_string_lossy().into_owned();
        let kind = entry.header().entry_type();
        if MEMBERS.contains(&name.as_str()) && kind.is_file() {
            let mode = if name == PROGRAM || name == INSTALLER { PROGRAM_MODE } else { MEMBER_MODE };
            let mut out = std::fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(p.staging.join(&name))?;
            let copied = std::io::copy(&mut (&mut entry).take(MAX_MEMBER_BYTES + 1), &mut out)?;
            ensure!(copied <= MAX_MEMBER_BYTES, "{name} in the package is too large");
        } else if (name == KERNEL || name == ROOTFS) && kind.is_symlink() {
            let target = entry.link_name()?.context("a link with no target")?.into_owned();
            ensure!(target.components().all(|c| matches!(c, Component::Normal(_) | Component::ParentDir)), "{name} links outside the release: {}", target.display());
            links.push((name, p.links_from.join(target)));
        } else if (name == KERNEL || name == ROOTFS) && kind.is_file() {
            entry.unpack(p.staging.join(format!("{name}.from-package")))?;
            links.push((name.clone(), p.staging.join(format!("{name}.from-package"))));
        }
    }
    for m in MEMBERS {
        ensure!(p.staging.join(m).is_file(), "the release has no {m}");
    }
    let v = std::fs::read_to_string(p.staging.join(VERSION_FILE))?;
    ensure!(v.trim() == version, "the release holds {}, not {version}", v.trim());
    let hashes = std::fs::read_to_string(p.staging.join(HASHES))?;
    let wanted = |name: &str| -> Result<String> {
        hashes
            .lines()
            .find_map(|l| l.split_once(char::is_whitespace).filter(|(_, n)| n.trim().trim_start_matches('*') == name).map(|(h, _)| h.to_ascii_lowercase()))
            .with_context(|| format!("{HASHES} names no {name}"))
    };
    let image = |name: &str| -> Result<PathBuf> {
        let want = wanted(name)?;
        if installed_hash(p, name).as_deref() == Some(want.as_str()) {
            return Ok(p.images_dir.join(name));
        }
        let from = links.iter().find(|(n, _)| n == name).map(|(_, f)| f.clone()).with_context(|| format!("the release has no {name}"))?;
        let to = p.staging.join(name);
        let ok = Command::new("cp").arg("-L").arg("--sparse=always").arg("--").arg(&from).arg(&to).env_clear().env("PATH", SAFE_PATH).status().context("run cp")?;
        ensure!(ok.success(), "could not copy {}", from.display());
        let got = sha256_file(&to)?;
        if got != want {
            bail!("{} is not the {name} the release was signed with (sha256 {got}, signed {want})", from.display());
        }
        Ok(to)
    };
    let kernel = image(KERNEL)?;
    let rootfs = image(ROOTFS)?;
    Ok(Staged { version: version.into(), installer: p.staging.join(INSTALLER), program: p.staging.join(PROGRAM), kernel, rootfs })
}

/// The installed image's sha256: the base image's is its `base.id`.
fn installed_hash(p: &Paths, name: &str) -> Option<String> {
    let file = p.images_dir.join(name);
    if !file.is_file() {
        return None;
    }
    if name == ROOTFS {
        return std::fs::read_to_string(p.images_dir.join(BASE_ID)).ok().map(|s| s.trim().to_ascii_lowercase());
    }
    sha256_file(&file).ok()
}

/// Run the staged installer as root, with nothing from this environment.
pub fn run_installer(user: &str, s: &Staged) -> Result<()> {
    let status = Command::new("/bin/sh")
        .arg(&s.installer)
        .arg(&s.program)
        .arg(&s.kernel)
        .arg(&s.rootfs)
        .arg(user)
        .env_clear()
        .env("PATH", SAFE_PATH)
        .status()
        .context("run the installer")?;
    ensure!(status.success(), "the installer for {} failed", s.version);
    Ok(())
}

#[cfg(test)]
#[path = "self_update_tests.rs"]
mod tests;
