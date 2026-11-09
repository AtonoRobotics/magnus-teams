//! Runs the agent's work on the person's own computer, inside the folders
//! they granted. The server decides what may be attempted. This side decides
//! what a path really is, because only this side can see the symlinks.
mod capture;
mod confine;
mod presence;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 256 * 1024;
const EXEC_LIMIT: Duration = Duration::from_secs(120);
/// Where macOS keeps its confinement tool, by absolute path so nothing can stand in.
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
const RETRY_DELAY: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// The folders the person granted, each resolved once at startup.
#[derive(Clone, Debug)]
pub struct Folders(Vec<PathBuf>);

impl Folders {
    pub fn new(paths: &[PathBuf]) -> Result<Folders> {
        if paths.is_empty() {
            bail!("grant at least one folder");
        }
        let mut v = vec![];
        for p in paths {
            let real = p.canonicalize().with_context(|| format!("{}", p.display()))?;
            if !real.is_dir() || real.parent().is_none() {
                bail!("{} cannot be granted", real.display());
            }
            v.push(real);
        }
        Ok(Folders(v))
    }

    pub fn list(&self) -> Vec<String> {
        self.0.iter().map(|p| p.display().to_string()).collect()
    }

    /// What `path` really is, or a refusal if any part of it leaves the
    /// granted folders. Its last parts may not exist yet.
    pub fn resolve(&self, path: &Path) -> Result<PathBuf> {
        if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
            bail!("{} is not a plain absolute path", path.display());
        }
        let mut existing = path.to_path_buf();
        let mut missing = vec![];
        while existing.symlink_metadata().is_err() {
            let name = existing.file_name().ok_or_else(|| anyhow!("{} has no parent", path.display()))?.to_os_string();
            missing.push(name);
            if !existing.pop() {
                bail!("{} has no parent", path.display());
            }
        }
        // A dangling link fails here, which is the right answer: it points
        // somewhere that cannot be checked.
        let mut real = existing.canonicalize().map_err(|_| refused(path))?;
        if !self.0.iter().any(|f| real.starts_with(f)) {
            return Err(refused(path));
        }
        real.extend(missing.iter().rev());
        Ok(real)
    }

    fn first(&self) -> Result<&PathBuf> {
        self.0.first().ok_or_else(|| anyhow!("no folder is granted"))
    }
}

fn refused(path: &Path) -> anyhow::Error {
    anyhow!("{} is outside the folders this computer granted", path.display())
}

/// Where this computer connects and what it allows.
#[derive(Clone, Debug)]
pub struct Config {
    pub server: String,
    pub token: String,
    pub name: String,
    pub mode: String,
    pub folders: Folders,
    pub bwrap: PathBuf,
    /// While this file exists, the person has told the agent to keep off.
    pub pause_file: Option<PathBuf>,
}

/// One job, carried out. The answer is the agent's tool result.
pub async fn run_job(job: &Value, folders: &Folders, bwrap: &Path) -> (bool, String) {
    match job_result(job, folders, bwrap).await {
        Ok(text) => (true, text),
        Err(e) => (false, format!("{e:#}")),
    }
}

async fn job_result(job: &Value, folders: &Folders, bwrap: &Path) -> Result<String> {
    let path = job["path"].as_str().map(|p| folders.resolve(Path::new(p))).transpose()?;
    let need = || path.clone().ok_or_else(|| anyhow!("a path is required"));
    let content = job["content"].as_str().unwrap_or_default();
    match job["op"].as_str().unwrap_or_default() {
        "read" => read(&need()?),
        "write" => write_new(&need()?, content),
        "edit" => overwrite(&need()?, content),
        "delete" => delete(&need()?),
        "exec" => {
            let cwd = job["cwd"].as_str().map(|p| folders.resolve(Path::new(p))).transpose()?.map_or_else(|| folders.first().cloned(), Ok)?;
            exec(job["command"].as_str().context("a command is required")?, &cwd, folders, bwrap, job["network"].as_bool().unwrap_or(false)).await
        }
        "capture" => capture::capture(job["kind"].as_str().context("a kind is required")?, job["seconds"].as_u64(), folders).await,
        other => bail!("unknown job {other}"),
    }
}

fn read(path: &Path) -> Result<String> {
    let size = std::fs::metadata(path).with_context(|| path.display().to_string())?.len();
    if size > MAX_FILE_BYTES {
        bail!("{} is {size} bytes, over the limit of {MAX_FILE_BYTES}", path.display());
    }
    Ok(String::from_utf8_lossy(&std::fs::read(path)?).into_owned())
}

/// A write that cannot overwrite: this is the one a reviewer may clear alone.
fn write_new(path: &Path, content: &str) -> Result<String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path).with_context(|| format!("{} already exists or cannot be created", path.display()))?;
    f.write_all(content.as_bytes())?;
    Ok(format!("created {}", path.display()))
}

fn overwrite(path: &Path, content: &str) -> Result<String> {
    if !std::fs::metadata(path).with_context(|| path.display().to_string())?.is_file() {
        bail!("{} is not a file", path.display());
    }
    std::fs::write(path, content)?;
    Ok(format!("replaced {}", path.display()))
}

fn delete(path: &Path) -> Result<String> {
    if std::fs::metadata(path).with_context(|| path.display().to_string())?.is_dir() {
        std::fs::remove_dir(path).with_context(|| format!("{} is not empty", path.display()))?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(format!("deleted {}", path.display()))
}

/// The bubblewrap arguments for a command that sees the system read-only,
/// the granted folders, and nothing else: no home directory, no keychain and
/// no network.
async fn exec(command: &str, cwd: &Path, folders: &Folders, bwrap: &Path, network: bool) -> Result<String> {
    let mut cmd = if cfg!(target_os = "linux") {
        let mut c = tokio::process::Command::new(bwrap);
        c.args(confine::bubblewrap_args(cwd, folders, command, network));
        c
    } else if cfg!(target_os = "macos") {
        let mut c = tokio::process::Command::new(SANDBOX_EXEC);
        c.args(["-p", &confine::seatbelt_profile(folders, network), "/bin/sh", "-c", command]).current_dir(cwd);
        c
    } else {
        bail!("commands cannot be confined on this operating system yet, so they do not run");
    };
    cmd.env_clear().env("PATH", "/usr/bin:/bin").env("HOME", cwd).stdin(Stdio::null()).kill_on_drop(true);
    let out = tokio::time::timeout(EXEC_LIMIT, cmd.output())
        .await
        .map_err(|_| anyhow!("the command ran past {EXEC_LIMIT:?} and was stopped"))?
        .context("the confinement did not start")?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    if text.len() > MAX_OUTPUT_BYTES {
        text = text.chars().take(MAX_OUTPUT_BYTES).collect();
    }
    if !out.status.success() {
        bail!("exit {}: {text}", out.status.code().unwrap_or(-1));
    }
    Ok(text)
}

/// Connects and works through jobs one at a time until the process ends.
/// One at a time also means no job's symlink can race another job's check.
pub async fn serve(cfg: Config) -> Result<()> {
    let http = reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    let register = json!({ "name": cfg.name, "mode": cfg.mode, "folders": cfg.folders.list() });
    let reply: Value = http.post(format!("{}/v1/local/computers", cfg.server)).bearer_auth(&cfg.token).json(&register).send().await?.error_for_status()?.json().await?;
    let id = reply["id"].as_str().context("the server gave no id")?.to_string();
    loop {
        if let Err(e) = next(&http, &cfg, &id).await {
            eprintln!("magnus-local: {e:#}");
            tokio::time::sleep(RETRY_DELAY).await;
        }
    }
}

async fn next(http: &reqwest::Client, cfg: &Config, id: &str) -> Result<()> {
    let active = presence::in_use(cfg.pause_file.as_deref());
    let r = http.get(format!("{}/v1/local/computers/{id}/next?active={active}", cfg.server)).bearer_auth(&cfg.token).send().await?.error_for_status()?;
    if r.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(());
    }
    let job: Value = r.json().await?;
    // Between asking and being given it, the person may have sat down.
    if job["resumed"] != json!(true) && presence::in_use(cfg.pause_file.as_deref()) {
        http.post(format!("{}/v1/local/computers/{id}/result", cfg.server))
            .bearer_auth(&cfg.token)
            .json(&json!({ "job": job["id"], "ok": false, "text": "", "busy": true }))
            .send()
            .await?
            .error_for_status()?;
        return Ok(());
    }
    let (ok, text) = run_job(&job, &cfg.folders, &cfg.bwrap).await;
    let done = json!({ "job": job["id"], "ok": ok, "text": text });
    http.post(format!("{}/v1/local/computers/{id}/result", cfg.server)).bearer_auth(&cfg.token).json(&done).send().await?.error_for_status()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf, Folders) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let granted = root.join("granted");
        let outside = root.join("outside");
        std::fs::create_dir_all(&granted).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let folders = Folders::new(std::slice::from_ref(&granted)).unwrap();
        (dir, granted, outside, folders)
    }

    #[test]
    fn a_path_inside_resolves_even_before_it_exists() {
        let (_d, granted, _o, f) = setup();
        assert_eq!(f.resolve(&granted.join("a/b.txt")).unwrap(), granted.join("a/b.txt"));
    }

    #[test]
    fn leaving_the_folder_is_refused_every_way() {
        let (_d, granted, outside, f) = setup();
        std::fs::write(outside.join("secret"), "x").unwrap();
        symlink(&outside, granted.join("dir")).unwrap();
        symlink(outside.join("secret"), granted.join("file")).unwrap();
        symlink(outside.join("gone"), granted.join("dangling")).unwrap();
        for p in [
            outside.join("secret"),
            granted.join("../outside/secret"),
            granted.join("dir/secret"),
            granted.join("dir/new"),
            granted.join("file"),
            granted.join("dangling"),
            PathBuf::from("relative"),
        ] {
            assert!(f.resolve(&p).is_err(), "{} was allowed", p.display());
        }
    }

    #[test]
    fn a_link_inside_the_folder_that_stays_inside_works() {
        let (_d, granted, _o, f) = setup();
        std::fs::write(granted.join("real"), "x").unwrap();
        symlink(granted.join("real"), granted.join("link")).unwrap();
        assert_eq!(f.resolve(&granted.join("link")).unwrap(), granted.join("real"));
    }

    #[test]
    fn a_folder_that_is_the_filesystem_root_cannot_be_granted() {
        assert!(Folders::new(&[PathBuf::from("/")]).is_err());
    }

    #[tokio::test]
    async fn a_new_file_never_overwrites() {
        let (_d, granted, _o, f) = setup();
        let target = granted.join("new.txt");
        let job = json!({ "op": "write", "path": target, "content": "one" });
        assert!(run_job(&job, &f, Path::new("bwrap")).await.0);
        let again = json!({ "op": "write", "path": target, "content": "two" });
        assert!(!run_job(&again, &f, Path::new("bwrap")).await.0);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "one");
    }

    #[test]
    fn a_command_sees_only_the_system_and_the_granted_folders() {
        let (_d, granted, _o, f) = setup();
        let args = confine::bubblewrap_args(&granted, &f, "ls", false);
        assert!(args.contains(&"--unshare-all".to_string()));
        let binds: Vec<&String> = args.windows(2).filter(|w| w[0] == "--bind").map(|w| &w[1]).collect();
        assert_eq!(binds, vec![&granted.display().to_string()]);
    }
}
