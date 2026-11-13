//! Signed appliance updates with one-step rollback (gate item 5).
//!
//! Releases arrive in `[updates] dir` (CI on the build box writes them
//! there): `appliance/latest.json` (or, on the channel an admin chose for
//! the server, `appliance/test/` or `appliance/development/`: see
//! `admin_app::server_channel`) names a package (a .tar.gz of the
//! programs and their `VERSION`) and its signature, made by `tauri signer
//! sign --app-version`, so the version is inside what was signed. The
//! appliance installs one only when the signature verifies with the release
//! key and binds the version the manifest announces, and only when it is
//! newer than what runs and was not rolled back from.
//!
//! Installing: unpack beside the programs' directory, run the new programs'
//! `check` against this appliance's log and vault (they must read both),
//! suspend running tasks ("update"), wait for their current step, swap the
//! directories (the old one is kept as `<dir>.prev`), and replace this
//! process with the new programs (`exec`, the same process id and
//! arguments). The new release resumes the tasks once it is serving. If it
//! cannot start, it swaps back and runs the old release, which resumes them.
//! The owner can go back one release the same way.
//!
//! The desktop app's packages are served to the apps, which check their own
//! signatures, from three release channels (DECISIONS.md, "Release
//! channels"): development (pull requests' builds, not merged;
//! `desktop/development/`), test (master, merged; `desktop/test/`) and
//! production (tagged releases; `desktop/`). The appliance decides which an
//! app gets, by its session: the owner's apps development, testers' test,
//! everyone else's production, and production's build only while the
//! app's channel has none. An app only ever moves forward.

use crate::config::Config;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::Status;
use anyhow::{bail, Context, Result};
use base64::Engine;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// The oldest desktop app this appliance serves: older apps are told to
/// update. Raise it with any API change an older app would get wrong.
pub const MIN_APP: &str = "0.5.0";

/// What an install leaves for the release that starts next.
const PENDING: &str = "update.json";

/// How long running tasks get to finish their current step before the
/// switch. Every step survives a restart at any point (the invariants), so
/// waiting only saves repeating a model reply in flight; a minute covers one.
const DRAIN: Duration = Duration::from_secs(60);
/// How often the drain looks for runs still driving.
const DRAIN_POLL: Duration = Duration::from_millis(100);

/// The directory the running programs were started from. Fixed at the
/// first look: once the directory is moved aside the kernel's link to the
/// running program follows it, and the path is what an update replaces.
pub fn install_dir() -> Result<PathBuf> {
    static D: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    D.get_or_init(|| std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf))).clone().context("the program has no directory")
}

/// The install directory, when it is one an installer made: its own, with
/// the release's VERSION. Updates move the whole directory aside, so they
/// never run from a shared one (/usr/local/bin, a build tree).
fn installed_dir() -> Result<PathBuf> {
    let dir = install_dir()?;
    anyhow::ensure!(dir.join("VERSION").is_file(), "{} is not an installed release (no VERSION beside the programs); updates are installed by hand here", dir.display());
    Ok(dir)
}

/// Exchange the running programs with the kept release (both ways: a
/// rollback, or undoing a switch).
fn swap_with_prev(dir: &Path) -> Result<()> {
    let prev = sibling(dir, ".prev");
    let tmp = sibling(dir, ".swap");
    swap(dir, &prev, &tmp)?;
    std::fs::rename(&tmp, &prev)?;
    Ok(())
}

fn sibling(dir: &Path, suffix: &str) -> PathBuf {
    let mut s = dir.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// The release that runs: the installed package's `VERSION`, or the
/// version the programs were built as when they were not installed from one.
pub fn version() -> String {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        install_dir()
            .ok()
            .and_then(|d| std::fs::read_to_string(d.join("VERSION")).ok())
            .map(|v| v.trim().to_string())
            .filter(|v| semver::Version::parse(v).is_ok())
            .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
    })
    .clone()
}

pub fn newer(a: &str, than: &str) -> bool {
    match (semver::Version::parse(a), semver::Version::parse(than)) {
        (Ok(a), Ok(b)) => a > b,
        _ => false,
    }
}

/// The signature must verify with the release key and bind `version`.
pub fn verify(data: &[u8], signature_b64: &str, public_key_b64: &str, version: &str) -> Result<()> {
    let text = |b: &str| -> Result<String> { Ok(String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b.trim())?)?) };
    let pk = minisign_verify::PublicKey::decode(&text(public_key_b64).context("the release key")?).map_err(|e| anyhow::anyhow!("the release key: {e}"))?;
    let sig = minisign_verify::Signature::decode(&text(signature_b64).context("the signature")?).map_err(|e| anyhow::anyhow!("the signature: {e}"))?;
    pk.verify(data, &sig, false).map_err(|_| anyhow::anyhow!("the signature does not verify with the release key"))?;
    let signed = sig.trusted_comment().split('\t').find_map(|f| f.strip_prefix("version:")).context("the signature names no version")?;
    anyhow::ensure!(signed == version, "the package was signed as {signed}, not {version}");
    Ok(())
}

/// `magnus check --config`: read this appliance's log and vault without
/// taking them over, as a new release must before it is switched to.
pub async fn check(cfg: &Config) -> Result<Value> {
    let events = crate::log::Log::read_only(&cfg.database.url).await?.load().await?;
    let mut state = crate::state::State::default();
    for e in &events {
        state.apply(e);
    }
    let vault = crate::vault::Vault::open(&cfg.appliance.state_dir)?;
    let secrets = vault.names()?.len();
    Ok(json!({ "version": version(), "events": events.len(), "secrets": secrets }))
}

fn pending_path(cfg: &Config) -> PathBuf {
    cfg.appliance.state_dir.join(PENDING)
}

/// A switch of releases is under way.
pub fn switching(cfg: &Config) -> bool {
    pending_path(cfg).exists()
}

pub fn pending(cfg: &Config) -> Option<Value> {
    std::fs::read(pending_path(cfg)).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

fn write_pending(cfg: &Config, v: &Value) -> Result<()> {
    std::fs::write(pending_path(cfg), serde_json::to_vec(v)?)?;
    Ok(())
}

/// How this process was started: the program's name and its arguments.
fn started_as() -> Result<Value> {
    let exe = std::env::current_exe()?;
    let program = exe.file_name().context("program name")?.to_string_lossy().to_string();
    let program = program.strip_suffix(" (deleted)").unwrap_or(&program).to_string();
    Ok(json!({ "program": program, "args": std::env::args().skip(1).collect::<Vec<_>>() }))
}

/// Replace this process with `program` from the install directory, which
/// now holds the other release (the same process id).
fn exec_installed(how: &Value) -> Result<()> {
    use std::os::unix::process::CommandExt;
    // The harness holds this socket for the whole test. The process that
    // replaces this one has to inherit it, or it binds the port and finds
    // the harness already there.
    crate::cli::release_listen_socket_for_exec();
    let path = install_dir()?.join(how["program"].as_str().context("program name")?);
    let args: Vec<String> = how["args"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(String::from)).collect();
    let e = std::process::Command::new(&path).args(&args).exec();
    Err(anyhow::anyhow!("start {}: {e}", path.display()))
}

/// Start this release again in this process (same pid and arguments), as
/// when the log may hold an event its state lacks; if that fails, exit, so
/// that nothing goes on from a state the log does not have.
pub fn restart_in_place() -> ! {
    let e = started_as().and_then(|how| exec_installed(&how));
    tracing::error!("restart in place: {e:?}");
    std::process::exit(1)
}

/// Exchange the running programs with the kept release.
fn swap(dir: &Path, with: &Path, keep_as: &Path) -> Result<()> {
    if keep_as.exists() {
        std::fs::remove_dir_all(keep_as)?;
    }
    std::fs::rename(dir, keep_as).with_context(|| format!("move {} aside", dir.display()))?;
    if let Err(e) = std::fs::rename(with, dir) {
        let _ = std::fs::rename(keep_as, dir);
        return Err(e).with_context(|| format!("put {} in place", with.display()));
    }
    Ok(())
}

/// Before the kernel starts: a release that was switched to and did not
/// come up is switched away from, once.
pub fn on_start(cfg: &Config) -> Result<()> {
    install_dir()?;
    let Some(mut p) = pending(cfg) else {
        return Ok(());
    };
    if p["started"].as_bool() == Some(true) {
        return roll_back_now(cfg, &p, "the new release did not start");
    }
    p["started"] = json!(true);
    write_pending(cfg, &p)
}

/// The release just switched to failed to start: go back and run the old one.
pub fn roll_back_now(cfg: &Config, p: &Value, reason: &str) -> Result<()> {
    if p["rollback"].as_bool() == Some(true) && p["owner"].as_bool() != Some(true) {
        // The release we went back to failed too: stay down for a person.
        let _ = std::fs::remove_file(pending_path(cfg));
        bail!("the release rolled back to did not start either: {reason}");
    }
    let dir = install_dir()?;
    let prev = sibling(&dir, ".prev");
    anyhow::ensure!(prev.is_dir(), "no earlier release to go back to");
    // The release gone back to may be one from before the vault's entries
    // were bound to their names: it reads them as it wrote them.
    crate::vault::Vault::open(&cfg.appliance.state_dir)?.unbind_all()?;
    let failed = sibling(&dir, ".failed");
    swap(&dir, &prev, &failed)?;
    // The failed release stays as the one a rollback would reach.
    if prev.exists() {
        std::fs::remove_dir_all(&prev)?;
    }
    std::fs::rename(&failed, &prev)?;
    // The old release starts again exactly as it was started.
    write_pending(cfg, &json!({ "from": p["to"], "to": p["from"], "rollback": true, "reason": reason, "started_as": p["started_as"] }))?;
    tracing::error!("going back to {}: {reason}", p["from"]);
    exec_installed(&p["started_as"])
}

impl Kernel {
    /// Once serving: record how the switch went and resume the suspended tasks.
    pub async fn after_start(self: &Arc<Self>) -> Result<()> {
        if let Some(p) = pending(&self.cfg) {
            let (from, to) = (p["from"].as_str().unwrap_or_default().to_string(), p["to"].as_str().unwrap_or_default().to_string());
            if p["rollback"].as_bool() == Some(true) {
                let reason = p["reason"].as_str().unwrap_or("rolled back").to_string();
                self.emit("kernel", Ev::UpdateRolledBack { from, to, reason }).await?;
            } else {
                self.emit("kernel", Ev::UpdateFinished { from, to }).await?;
            }
            std::fs::remove_file(pending_path(&self.cfg))?;
        } else if let Some((_, to, _)) = self.read(|s| s.updating.clone()) {
            // Stopped between suspending the tasks and the switch.
            self.emit("kernel", Ev::UpdateFailed { to, reason: "the server stopped before the switch".into() }).await?;
        }
        self.resume_updated().await
    }

    async fn resume_updated(self: &Arc<Self>) -> Result<()> {
        if self.read(|s| s.kill_switch) {
            return Ok(());
        }
        let runs: Vec<String> = self.read(|s| s.runs.values().filter(|r| r.status == Status::Suspended && r.reason.as_deref() == Some("update")).map(|r| r.id.clone()).collect());
        for run in runs {
            self.emit("kernel", Ev::Resumed { run: run.clone() }).await?;
            self.wake(&run);
        }
        Ok(())
    }

    /// The latest release on the server's channel, if any.
    pub fn latest_release(&self) -> Option<Value> {
        std::fs::read(crate::admin_app::server_channel::release_dir(self).join("latest.json")).ok().and_then(|b| serde_json::from_slice(&b).ok())
    }

    /// Look for a newer release and install it. Returns what happened, for
    /// the owner; on success this process is replaced and never returns.
    pub async fn update(self: &Arc<Self>, actor: &str) -> Result<Value> {
        let _one = updating().lock().await;
        let current = version();
        let Some(m) = self.latest_release() else {
            return Ok(json!({ "status": "up_to_date", "version": current }));
        };
        let to = m["version"].as_str().unwrap_or_default().to_string();
        if !newer(&to, &current) || self.read(|s| s.skipped_releases.contains(&to)) {
            return Ok(json!({ "status": "up_to_date", "version": current, "latest": to }));
        }
        let new = match self.prepare(&m, &to).await {
            Ok(dir) => dir,
            Err(e) => {
                let reason = format!("{e:#}");
                self.emit(actor, Ev::UpdateFailed { to: to.clone(), reason: reason.clone() }).await?;
                return Ok(json!({ "status": "failed", "version": current, "latest": to, "reason": reason }));
            }
        };
        self.switch(actor, &current, &to, false, |dir| swap(dir, &new, &sibling(dir, ".prev"))).await
    }

    /// Go back one release: the one kept beside the running programs. It
    /// proves it can read this appliance first, as an update does; if it
    /// still does not start, the appliance comes back to this release.
    pub async fn roll_back(self: &Arc<Self>, actor: &str) -> Result<Value> {
        let _one = updating().lock().await;
        let dir = installed_dir()?;
        let prev = sibling(&dir, ".prev");
        let to = std::fs::read_to_string(prev.join("VERSION")).map(|v| v.trim().to_string()).context("no earlier release is kept")?;
        let current = version();
        let exe = std::env::current_exe()?;
        let prog = prev.join(exe.file_name().context("program name")?);
        let out = tokio::process::Command::new(&prog).args(["check", "--config"]).arg(&self.cfg.path).output().await.context("run the earlier release's check")?;
        if !out.status.success() {
            let reason = format!("the earlier release cannot read this appliance: {}", String::from_utf8_lossy(&out.stderr).trim());
            self.emit(actor, Ev::UpdateFailed { to: to.clone(), reason: reason.clone() }).await?;
            return Ok(json!({ "status": "failed", "version": current, "latest": to, "reason": reason }));
        }
        self.switch(actor, &current, &to, true, swap_with_prev).await
    }

    /// Verify, unpack and check a release; its programs' directory.
    async fn prepare(&self, m: &Value, to: &str) -> Result<PathBuf> {
        let file = m["file"].as_str().context("the release names no file")?;
        anyhow::ensure!(!file.contains('/') && !file.contains(".."), "not a release file name");
        let bytes = std::fs::read(crate::admin_app::server_channel::release_dir(self).join(file)).with_context(|| format!("read {file}"))?;
        verify(&bytes, m["signature"].as_str().unwrap_or_default(), &self.cfg.updates.public_key, to)?;
        let dir = installed_dir()?;
        let new = sibling(&dir, ".new");
        if new.exists() {
            std::fs::remove_dir_all(&new)?;
        }
        std::fs::create_dir_all(&new)?;
        tar::Archive::new(flate2::read::GzDecoder::new(&bytes[..])).unpack(&new).context("unpack the release")?;
        let v = std::fs::read_to_string(new.join("VERSION")).context("the release has no VERSION")?;
        anyhow::ensure!(v.trim() == to, "the release holds {}, not {to}", v.trim());
        let exe = std::env::current_exe()?;
        let prog = new.join(exe.file_name().context("program name")?);
        anyhow::ensure!(prog.is_file(), "the release has no {}", prog.display());
        let out = tokio::process::Command::new(&prog).args(["check", "--config"]).arg(&self.cfg.path).output().await.context("run the new release's check")?;
        anyhow::ensure!(out.status.success(), "the new release cannot read this appliance: {}", String::from_utf8_lossy(&out.stderr).trim());
        Ok(new)
    }

    /// Suspend the running tasks, let them finish their step, swap the
    /// programs and start them in this process.
    async fn switch(self: &Arc<Self>, actor: &str, from: &str, to: &str, rollback: bool, put: impl FnOnce(&Path) -> Result<()>) -> Result<Value> {
        self.emit(actor, Ev::UpdateStarted { from: from.into(), to: to.into(), rollback }).await?;
        self.suspend_for_update().await?;
        let start = std::time::Instant::now();
        while self.any_driving() && start.elapsed() < DRAIN {
            tokio::time::sleep(DRAIN_POLL).await;
        }
        let dir = install_dir()?;
        let r = started_as().and_then(|how| {
            if rollback {
                // As roll_back_now: the earlier release reads the vault as it wrote it.
                self.vault.unbind_all()?;
            }
            write_pending(&self.cfg, &json!({ "from": from, "to": to, "rollback": rollback, "owner": rollback, "reason": if rollback { "the owner went back one release" } else { "" }, "started_as": how }))?;
            put(&dir)?;
            Ok(how)
        });
        let how = match r {
            Ok(how) => how,
            Err(e) => {
                let _ = std::fs::remove_file(pending_path(&self.cfg));
                self.rebind_after_failed_switch(rollback)?;
                let reason = format!("{e:#}");
                self.emit("kernel", Ev::UpdateFailed { to: to.into(), reason: reason.clone() }).await?;
                self.resume_updated().await?;
                return Ok(json!({ "status": "failed", "version": from, "latest": to, "reason": reason }));
            }
        };
        tracing::info!("switching from {from} to {to}");
        let e = exec_installed(&how).unwrap_err();
        // Still this release, running: put its programs back, and say so.
        let _ = std::fs::remove_file(pending_path(&self.cfg));
        let undone = swap_with_prev(&dir);
        self.rebind_after_failed_switch(rollback)?;
        let reason = format!("{e:#}{}", undone.err().map(|u| format!("; putting this release back failed too: {u:#}")).unwrap_or_default());
        self.emit("kernel", Ev::UpdateFailed { to: to.into(), reason: reason.clone() }).await?;
        self.resume_updated().await?;
        Ok(json!({ "status": "failed", "version": from, "latest": to, "reason": reason }))
    }

    /// Going back failed, so this release keeps running: its vault is bound
    /// again, and open unless the kill switch is engaged.
    fn rebind_after_failed_switch(&self, rollback: bool) -> Result<()> {
        if rollback {
            self.vault.bind_unbound()?;
            if !self.read(|s| s.kill_switch) {
                self.vault.unseal();
            }
        }
        Ok(())
    }

    /// Every running task waits for the switch.
    pub async fn suspend_for_update(&self) -> Result<()> {
        let running: Vec<String> = self.read(|s| s.runs.values().filter(|r| r.status == Status::Running).map(|r| r.id.clone()).collect());
        for run in running {
            self.suspend(&run, "update", None).await?;
        }
        Ok(())
    }
}

fn updating() -> &'static tokio::sync::Mutex<()> {
    static M: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    M.get_or_init(Default::default)
}

/// Look for a release every `check_minutes`, and install it when automatic.
pub fn schedule(k: Arc<Kernel>) {
    let c = k.cfg.updates.clone();
    if c.check_minutes == 0 || !c.automatic {
        return;
    }
    tokio::spawn(async move {
        let mut t = tokio::time::interval(Duration::from_secs(c.check_minutes as u64 * 60));
        loop {
            t.tick().await;
            match k.update("kernel:update").await {
                Ok(v) if v["status"] == "failed" => tracing::error!("update: {}", v["reason"]),
                Ok(_) => {}
                Err(e) => tracing::error!("update: {e:#}"),
            }
        }
    });
}

/// The desktop app's release channels.
pub const CHANNELS: [&str; 3] = ["development", "test", "production"];

fn channel_dir(cfg: &Config, c: &str) -> PathBuf {
    let desktop = cfg.updates_dir().join("desktop");
    if c == "production" {
        desktop
    } else {
        desktop.join(c)
    }
}

fn channel_release(cfg: &Config, c: &str) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(channel_dir(cfg, c).join("latest.json")).ok()?).ok()
}

/// The desktop app's release manifest for an app on `channel`: the
/// channel's build, or production's while the channel has none, with each
/// package's address on this appliance (`origin` is the address the app
/// used).
pub fn desktop_manifest(cfg: &Config, origin: &str, channel: &str) -> Option<Value> {
    // Not production's when it is newer: it need not hold the channel's work
    // (a pull request not yet merged), and an app never loses what it has.
    let channel = if channel_release(cfg, channel).is_some() { channel } else { "production" };
    let mut m = channel_release(cfg, channel)?;
    let files = if channel == "production" { "files".to_string() } else { format!("{channel}/files") };
    if let Some(platforms) = m["platforms"].as_object_mut() {
        for p in platforms.values_mut() {
            if let Some(f) = p["file"].as_str().map(String::from) {
                p["url"] = json!(format!("{origin}/v1/updates/desktop/{files}/{f}"));
            }
        }
    }
    Some(m)
}

/// The channel the app asking gets: development for the owner's apps, test
/// for testers' (the `tester` role), production for everyone else's. Only a
/// session says who asks. Without one that still holds (signed out, ended,
/// or an app from before channels), an app on a release gets production, and
/// one on a pre-release gets nothing until it asks signed in: its version is
/// only what the request says, so it chooses no channel (owner: "No one but
/// me should have a choice"), and production need not hold its build's work.
/// A pre-release older than `MIN_APP` cannot sign in at all, so it gets
/// production: everyone's builds, and its only way forward.
pub async fn assigned_channel(k: &Kernel, h: &HeaderMap) -> Option<&'static str> {
    // Everyone else's, as an admin set it (Operations › Updates).
    let everyone = k.read(|s| s.desktop_channel.clone()).and_then(|c| CHANNELS.iter().copied().find(|x| *x == c)).unwrap_or("production");
    if let Ok(p) = crate::api::who(k, h).await {
        return Some(if p.is_owner() {
            "development"
        } else if p.has_role("tester") {
            "test"
        } else {
            everyone
        });
    }
    let app = h.get("x-magnus-app").and_then(|v| v.to_str().ok()).and_then(|v| semver::Version::parse(v.trim()).ok());
    match app {
        Some(v) if !v.pre.is_empty() && !newer(MIN_APP, &v.to_string()) => None,
        _ => Some(everyone),
    }
}

/// The release kept to go back to, if there is one.
pub fn previous_version() -> Option<String> {
    install_dir().ok().and_then(|d| std::fs::read_to_string(sibling(&d, ".prev").join("VERSION")).ok()).map(|v| v.trim().to_string())
}

use crate::api::{err, internal, owner, Transport};
use axum::{
    extract::{Path as UrlPath, Request, State as AxState},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};

type K = AxState<Arc<Kernel>>;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/version", get(|| async { Json(json!({ "version": version(), "min_app": MIN_APP })) }))
        .route("/v1/updates", get(status))
        .route("/v1/updates/check", post(check_now))
        .route("/v1/updates/rollback", post(roll_back))
        .route("/v1/updates/desktop/latest.json", get(desktop_latest))
        .route("/v1/updates/desktop/files/{name}", get(production_file))
        .route("/v1/updates/desktop/{channel}/files/{name}", get(desktop_file))
}

/// Versions both ways: every answer names this release, and an app older
/// than `MIN_APP` (it says its own in `x-magnus-app`) is told to update.
pub async fn app_version(req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let exempt = path == "/v1/version" || path == "/v1/health" || path.starts_with("/v1/updates/desktop/");
    let app = req.headers().get("x-magnus-app").and_then(|v| v.to_str().ok()).map(String::from);
    let mut r = match app {
        Some(a) if !exempt && path.starts_with("/v1/") && newer(MIN_APP, &a) => (
            StatusCode::UPGRADE_REQUIRED,
            Json(json!({ "error": format!("This app ({a}) is older than this appliance accepts. Update the app to {MIN_APP} or later."), "update_required": true, "min_app": MIN_APP, "version": version() })),
        )
            .into_response(),
        _ => next.run(req).await,
    };
    if let Ok(v) = HeaderValue::from_str(&version()) {
        r.headers_mut().insert("x-magnus-appliance", v);
    }
    r
}

/// An app signing in without naming its version is older than 0.5.0 (the
/// first that says it): told to update instead of given a session it would
/// use with an API it gets wrong.
pub fn unversioned_app(h: &HeaderMap) -> Option<Response> {
    if h.contains_key("x-magnus-app") {
        return None;
    }
    Some(
        (
            StatusCode::UPGRADE_REQUIRED,
            Json(json!({ "error": format!("This app is older than this appliance accepts. Update the app to {MIN_APP} or later."), "update_required": true, "min_app": MIN_APP, "version": version() })),
        )
            .into_response(),
    )
}

async fn status(AxState(k): K, h: HeaderMap) -> Response {
    if let Err(r) = owner(&k, &h).await {
        return r;
    }
    let latest = k.latest_release().map(|m| json!({ "version": m["version"], "notes": m["notes"] }));
    let prev = install_dir().ok().and_then(|d| std::fs::read_to_string(sibling(&d, ".prev").join("VERSION")).ok()).map(|v| v.trim().to_string());
    let (updating, last, skipped) = k.read(|s| (s.updating.clone(), s.last_update.clone(), s.skipped_releases.clone()));
    Json(json!({
        "version": version(),
        "min_app": MIN_APP,
        "automatic": k.cfg.updates.automatic,
        "latest": latest,
        "updating": updating.map(|(from, to, rollback)| json!({ "from": from, "to": to, "rollback": rollback })),
        "last": last,
        "skipped": skipped,
        "previous": prev,
    }))
    .into_response()
}

/// Install the latest release now. The switch happens after the answer.
async fn check_now(AxState(k): K, h: HeaderMap) -> Response {
    let p = match owner(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let current = version();
    let to = k.latest_release().and_then(|m| m["version"].as_str().map(String::from));
    match to {
        Some(to) if newer(&to, &current) && !k.read(|s| s.skipped_releases.contains(&to)) => {
            let actor = format!("person:{}", p.id);
            tokio::spawn(async move {
                if let Err(e) = k.update(&actor).await {
                    tracing::error!("update: {e:#}");
                }
            });
            (StatusCode::ACCEPTED, Json(json!({ "status": "installing", "version": current, "latest": to }))).into_response()
        }
        to => Json(json!({ "status": "up_to_date", "version": current, "latest": to })).into_response(),
    }
}

async fn roll_back(AxState(k): K, h: HeaderMap) -> Response {
    let p = match owner(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(to) = install_dir().ok().and_then(|d| std::fs::read_to_string(sibling(&d, ".prev").join("VERSION")).ok()) else {
        return err(StatusCode::CONFLICT, "no earlier release is kept on this server");
    };
    let actor = format!("person:{}", p.id);
    tokio::spawn(async move {
        if let Err(e) = k.roll_back(&actor).await {
            tracing::error!("roll back: {e:#}");
        }
    });
    (StatusCode::ACCEPTED, Json(json!({ "status": "rolling_back", "version": version(), "to": to.trim() }))).into_response()
}

/// The apps ask with their session if they have one; without, an app too old
/// to sign in can still update. The appliance decides the channel.
async fn desktop_latest(AxState(k): K, Extension(t): Extension<Transport>, h: HeaderMap) -> Response {
    let Some(channel) = assigned_channel(&k, &h).await else {
        return StatusCode::NO_CONTENT.into_response();
    };
    let Some(host) = h.get("host").and_then(|v| v.to_str().ok()) else {
        return err(StatusCode::BAD_REQUEST, "no host");
    };
    let scheme = if t == Transport::Tls { "https" } else { "http" };
    match desktop_manifest(&k.cfg, &format!("{scheme}://{host}"), channel) {
        Some(m) => Json(m).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

/// Production's packages, at the address apps before channels were given.
async fn production_file(k: K, h: HeaderMap, UrlPath(name): UrlPath<String>) -> Response {
    desktop_file(k, h, UrlPath(("production".into(), name))).await
}

/// A channel's package. Production's are anyone's; development's and test's
/// only the apps given that channel (the updater asks for the package with
/// the session it asked for the manifest with).
async fn desktop_file(AxState(k): K, h: HeaderMap, UrlPath((channel, name)): UrlPath<(String, String)>) -> Response {
    if !CHANNELS.contains(&channel.as_str()) || name.contains('/') || name.contains("..") || name == "latest.json" {
        return err(StatusCode::NOT_FOUND, "no such file");
    }
    if channel != "production" && assigned_channel(&k, &h).await != Some(channel.as_str()) {
        return err(StatusCode::NOT_FOUND, "no such file");
    }
    match tokio::fs::read(channel_dir(&k.cfg, &channel).join(&name)).await {
        Ok(b) => ([("content-type", "application/octet-stream")], b).into_response(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => err(StatusCode::NOT_FOUND, "no such file"),
        Err(e) => internal(e.into()),
    }
}
