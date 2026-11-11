//! Whether the computer service is as new as this server
//! (`decisions/2026-11-11-computerd-version-and-update.md`). The server
//! updates itself; computerd is root's, and only root updates it. When it
//! stays behind, a request it does not know comes back "bad request" (the
//! owner's Reset, 2026-11-11). So the kernel asks computerd's `version` at
//! start and every few minutes, shows Admin › Operations when it is behind,
//! with the command that updates it, and refuses what it cannot serve in
//! plain words instead of passing "bad request" on.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// How often the kernel asks computerd what it is.
pub const CHECK_EVERY: Duration = Duration::from_secs(5 * 60);
/// What a person is told when computerd cannot do what they asked.
pub const OUTDATED: &str = "The computer service on this server is older than Magnus and cannot do this until it is updated. An administrator can see how in Admin › Operations.";
/// What Admin › Operations says while it is behind.
pub const BEHIND: &str = "The computer service is older than this server: computers may fail to start or reset until it is updated.";
/// The sha256 of the release's images, beside the programs.
const IMAGE_HASHES: &str = "images.sha256";
const BASE_IMAGE: &str = "rootfs.ext4";
/// Computerd's words for a request it cannot read.
const BAD_REQUEST: &str = "bad request";

/// What computerd said it is.
#[derive(Clone, Debug, PartialEq)]
pub enum Seen {
    Known {
        version: String,
        base_id: Option<String>,
        ops: Vec<String>,
    },
    /// It refused `version`: built before computerd could say.
    Old,
}

/// The computerd this server shipped with.
#[derive(Clone, Debug, PartialEq)]
pub struct Shipped {
    pub version: String,
    pub base_id: Option<String>,
}

fn seen() -> &'static Mutex<BTreeMap<PathBuf, Seen>> {
    static S: std::sync::OnceLock<Mutex<BTreeMap<PathBuf, Seen>>> = std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

pub fn last_seen(socket: &Path) -> Option<Seen> {
    seen().lock().ok().and_then(|m| m.get(socket).cloned())
}

fn remember(socket: &Path, s: Seen) {
    if let Ok(mut m) = seen().lock() {
        m.insert(socket.to_path_buf(), s);
    }
}

pub fn is_bad_request(e: &anyhow::Error) -> bool {
    e.to_string().starts_with(BAD_REQUEST)
}

/// What a reply to `version` means.
pub fn read_reply(reply: Result<Value>) -> Result<Seen> {
    match reply {
        Ok(v) => Ok(Seen::Known {
            version: v["version"].as_str().unwrap_or_default().to_string(),
            base_id: v["base_id"].as_str().map(str::to_string),
            ops: v["ops"].as_array().map(|a| a.iter().filter_map(|o| o.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        }),
        Err(e) if is_bad_request(&e) => Ok(Seen::Old),
        Err(e) => Err(e),
    }
}

/// Ask computerd what it is, and remember it.
pub async fn check(socket: &Path) -> Result<Seen> {
    let s = read_reply(super::client::ask_unchecked(socket, json!({ "op": "version" })).await)?;
    remember(socket, s.clone());
    Ok(s)
}

/// The release's base image id, from the hashes signed with it.
pub fn shipped_base_id(install_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(install_dir.join(IMAGE_HASHES)).ok()?;
    text.lines().find_map(|l| l.split_once(char::is_whitespace).filter(|(_, n)| n.trim().trim_start_matches('*') == BASE_IMAGE).map(|(h, _)| h.to_ascii_lowercase()))
}

pub fn shipped() -> Shipped {
    Shipped { version: crate::update::version(), base_id: crate::update::install_dir().ok().and_then(|d| shipped_base_id(&d)) }
}

/// Older than the one shipped: an older release, or the same release with
/// another base image (the images were not installed with it).
pub fn behind(s: &Seen, shipped: &Shipped) -> bool {
    match s {
        Seen::Old => true,
        Seen::Known { version, base_id, .. } => {
            crate::update::newer(&shipped.version, version) || (version == &shipped.version && shipped.base_id.is_some() && base_id != &shipped.base_id)
        }
    }
}

/// Whether computerd is known not to serve `op`: it listed what it serves,
/// without it. One that has not said, or is too old to say, is sent the
/// request, and its "bad request" is put in plain words.
pub fn lacks(s: Option<&Seen>, op: &str) -> bool {
    matches!(s, Some(Seen::Known { ops, .. }) if !ops.iter().any(|o| o == op))
}

/// Whether computerd is known to serve `op`. A caller that records a change
/// before asking computerd to make it (a reset that erases the files) asks
/// this first, so the log never says what computerd then refuses.
pub fn can_surely(s: &Seen, op: &str) -> bool {
    matches!(s, Seen::Known { ops, .. } if ops.iter().any(|o| o == op))
}

/// Whether computerd surely serves `op`. What it said last is taken only
/// when it says yes; otherwise it is asked again, since it may have been
/// updated since.
pub async fn serves(socket: &Path, op: &str) -> Result<bool> {
    if last_seen(socket).is_some_and(|s| can_surely(&s, op)) {
        return Ok(true);
    }
    Ok(can_surely(&check(socket).await?, op))
}

/// Whether computerd is known not to serve `op`, asked again before a no.
pub async fn known_to_lack(socket: &Path, op: &str) -> bool {
    if !lacks(last_seen(socket).as_ref(), op) {
        return false;
    }
    check(socket).await.is_ok_and(|s| lacks(Some(&s), op))
}

pub fn outdated() -> anyhow::Error {
    anyhow!(OUTDATED)
}

fn quoted(p: &str) -> String {
    if !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-+@:=".contains(&b)) {
        p.to_string()
    } else {
        format!("'{}'", p.replace('\'', r"'\''"))
    }
}

/// The command that installs the computerd this server shipped with and
/// sets up its own updates from the signed releases.
pub fn update_command(install_dir: &Path, kernel: &Path, rootfs: &Path, user: &str, releases: &Path) -> String {
    let s = |p: &Path| quoted(&p.display().to_string());
    format!("sudo sh {} {} {} {} {} {}", s(&install_dir.join("install-computerd.sh")), s(&install_dir.join("magnus-computerd")), s(kernel), s(rootfs), quoted(user), s(releases))
}

/// The user this server runs as.
fn user_name() -> String {
    let uid = unsafe { libc::getuid() };
    let mut buf = vec![0 as libc::c_char; 1024];
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut out: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: every pointer is to a live, writable buffer of the given size.
    let rc = unsafe { libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut out) };
    if rc != 0 || out.is_null() {
        return uid.to_string();
    }
    // SAFETY: getpwuid_r succeeded, so pw_name is a NUL-terminated string in `buf`.
    unsafe { std::ffi::CStr::from_ptr(pwd.pw_name) }.to_string_lossy().into_owned()
}

/// For Admin › Operations: what computerd is, against what was shipped.
pub fn report(cfg: &crate::config::Config, socket: &Path) -> Value {
    let shipped = shipped();
    let Some(s) = last_seen(socket) else {
        return json!({ "checked": false, "behind": false, "expected": shipped.version });
    };
    let version = match &s {
        Seen::Known { version, .. } => Some(version.clone()),
        Seen::Old => None,
    };
    if !behind(&s, &shipped) {
        return json!({ "checked": true, "behind": false, "version": version, "expected": shipped.version });
    }
    let dir = crate::update::install_dir().unwrap_or_default();
    let (kernel, rootfs) = cfg.sandbox.as_ref().map_or_else(|| (dir.join("vmlinux"), dir.join(BASE_IMAGE)), |s| (s.kernel_image.clone(), s.rootfs.clone()));
    let command = update_command(&dir, &kernel, &rootfs, &user_name(), &cfg.updates_dir().join("appliance"));
    json!({ "checked": true, "behind": true, "version": version, "expected": shipped.version, "message": BEHIND, "command": command })
}

/// At start and every `CHECK_EVERY`; a change is logged once.
pub fn schedule(socket: PathBuf) {
    tokio::spawn(async move {
        let mut was: Option<bool> = None;
        loop {
            match check(&socket).await {
                Ok(s) => {
                    let now = behind(&s, &shipped());
                    if now && was != Some(true) {
                        tracing::warn!("{BEHIND} It says {s:?}; this server shipped {:?}.", shipped());
                    }
                    was = Some(now);
                }
                Err(e) => tracing::debug!("could not ask the computer service its version: {e:#}"),
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

#[cfg(test)]
#[path = "service_version_tests.rs"]
mod tests;
