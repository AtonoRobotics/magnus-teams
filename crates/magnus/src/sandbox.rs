//! One Firecracker microVM per run, with no network device, controlled over
//! vsock (K16). The workspace disk outlives a stopped VM so a resumed run
//! boots a fresh VM on the same disk (K20); it is deleted with the run (K19).
//! A contract check runs in a VM of its own, booted from the image with a
//! throwaway copy of the workspace, so neither the work nor an earlier check
//! can change what the checking tool is (K42).

use crate::config::Sandbox as Cfg;
use anyhow::{anyhow, Context, Result};
use base64::Engine;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

const GUEST_PORT: u32 = 52;

/// How long past a command's own timeout its VM has to reply.
const EXEC_REPLY_GRACE_SECONDS: u64 = 15;
/// How long one file read or write in a VM may take.
const FILE_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// How long one ping of a booting guest may take.
const PING_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a VM has to come up.
const BOOT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the image's check-tool resolver may run.
const CHECK_TOOLS_TIMEOUT_SECONDS: u64 = 60;
/// How long a stopping VM has to flush its disk.
const SYNC_TIMEOUT: Duration = Duration::from_secs(5);

/// How long to wait for the reply to a command that may run `timeout_s`:
/// its timeout and a grace, never wrapping, whatever the caller asked.
fn exec_reply_timeout(timeout_s: u64) -> Duration {
    Duration::from_secs(timeout_s.saturating_add(EXEC_REPLY_GRACE_SECONDS))
}

/// What the image's resolver found in one check command.
#[derive(Debug, serde::Deserialize)]
pub struct Resolved {
    pub problems: Vec<String>,
    pub self_checked: Vec<String>,
}

pub struct Vm {
    child: Mutex<Child>,
    vsock: PathBuf,
}

impl Drop for Vm {
    fn drop(&mut self) {
        let mut c = self.child.lock().unwrap();
        let _ = c.kill();
        let _ = c.wait();
    }
}

pub struct Sandboxes {
    cfg: Option<Cfg>,
    root: PathBuf,
    vms: tokio::sync::Mutex<HashMap<String, Arc<Vm>>>,
    inputs: Mutex<HashMap<String, Vec<(String, Vec<u8>)>>>,
}

/// The Firecracker configuration for a run's VM, or for a check's (`check`
/// tells the guest to run commands in the check environment). It never has a
/// network interface; `launch` refuses one even if a caller adds it.
pub fn vm_config(cfg: &Cfg, workspace: &Path, vsock: &Path, check: bool) -> Value {
    let mut args = "console=ttyS0 reboot=k panic=1 pci=off quiet loglevel=3 init=/sbin/magnus-guest".to_string();
    if check {
        args.push_str(" magnus.check=1");
    }
    json!({
        "boot-source": {
            "kernel_image_path": cfg.kernel_image,
            "boot_args": args
        },
        "drives": [
            { "drive_id": "rootfs", "path_on_host": cfg.rootfs, "is_root_device": true, "is_read_only": true },
            { "drive_id": "workspace", "path_on_host": workspace, "is_root_device": false, "is_read_only": false }
        ],
        "machine-config": { "vcpu_count": cfg.vcpus, "mem_size_mib": cfg.memory_mib },
        "vsock": { "guest_cid": 3, "uds_path": vsock }
    })
}

impl Sandboxes {
    pub fn new(cfg: Option<Cfg>, state_dir: &Path) -> Sandboxes {
        Sandboxes { cfg, root: state_dir.join("runs"), vms: tokio::sync::Mutex::new(HashMap::new()), inputs: Mutex::new(HashMap::new()) }
    }

    pub fn available(&self) -> bool {
        self.cfg.is_some()
    }

    fn dir(&self, run: &str) -> PathBuf {
        self.root.join(run)
    }

    /// A folder for a check VM, short enough for the VM's socket paths.
    fn scratch_dir(&self) -> PathBuf {
        self.root.join(format!("chk-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]))
    }

    async fn launch(&self, run: &str) -> Result<Arc<Vm>> {
        self.launch_in(&self.dir(run), false).await
    }

    /// Boot a VM whose files live in `dir`, creating its workspace disk if
    /// there is none.
    async fn launch_in(&self, dir: &Path, check: bool) -> Result<Arc<Vm>> {
        let cfg = self.cfg.as_ref().ok_or_else(|| anyhow!("no sandbox is configured on this server"))?;
        std::fs::create_dir_all(dir)?;
        let workspace = dir.join("workspace.ext4");
        if !workspace.exists() {
            let f = std::fs::File::create(&workspace)?;
            f.set_len(cfg.workspace_mib as u64 * 1024 * 1024)?;
            drop(f);
            let out = Command::new("mkfs.ext4")
                .args(["-q", "-F", "-L", "workspace"])
                .arg(&workspace)
                .output()
                .or_else(|_| Command::new("/usr/sbin/mkfs.ext4").args(["-q", "-F", "-L", "workspace"]).arg(&workspace).output())
                .context("run mkfs.ext4")?;
            anyhow::ensure!(out.status.success(), "mkfs.ext4 failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        let vsock = dir.join("vsock.sock");
        let api = dir.join("api.sock");
        let _ = std::fs::remove_file(&vsock);
        let _ = std::fs::remove_file(&api);
        let config = vm_config(cfg, &workspace, &vsock, check);
        anyhow::ensure!(config.get("network-interfaces").is_none(), "refusing a sandbox with a network device");
        let config_path = dir.join("vm.json");
        std::fs::write(&config_path, serde_json::to_vec_pretty(&config)?)?;
        let console = std::fs::File::create(dir.join("console.log"))?;
        let mut cmd = Command::new(&cfg.firecracker);
        cmd.arg("--api-sock").arg(&api).arg("--config-file").arg(&config_path).arg("--level").arg("Error");
        cmd.env_clear();
        cmd.stdin(Stdio::null()).stdout(Stdio::from(console.try_clone()?)).stderr(Stdio::from(console));
        let child = crate::spawn::spawn(cmd).context("start firecracker")?;
        let vm = Arc::new(Vm { child: Mutex::new(child), vsock });
        // Wait for the guest agent.
        let start = std::time::Instant::now();
        loop {
            if let Ok(v) = request(&vm.vsock, &json!({ "op": "ping" }), PING_TIMEOUT).await {
                if v["ok"] == true {
                    return Ok(vm);
                }
            }
            if let Ok(Some(st)) = vm.child.lock().unwrap().try_wait() {
                let log = std::fs::read_to_string(dir.join("console.log")).unwrap_or_default();
                return Err(anyhow!("the sandbox VM exited during boot ({st}): {}", log.chars().rev().take(800).collect::<String>().chars().rev().collect::<String>()));
            }
            if start.elapsed() > BOOT_TIMEOUT {
                return Err(anyhow!("the sandbox VM did not come up within {}s", BOOT_TIMEOUT.as_secs()));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Files to place in /workspace/inputs when a run's workspace is created.
    pub fn set_inputs(&self, run: &str, files: Vec<(String, Vec<u8>)>) {
        self.inputs.lock().unwrap().insert(run.to_string(), files);
    }

    async fn vm(&self, run: &str) -> Result<Arc<Vm>> {
        let mut vms = self.vms.lock().await;
        if let Some(vm) = vms.get(run) {
            if vm.child.lock().unwrap().try_wait().ok().flatten().is_none() {
                return Ok(vm.clone());
            }
        }
        let fresh = !self.dir(run).join("workspace.ext4").exists();
        let vm = self.launch(run).await?;
        if fresh {
            let files = self.inputs.lock().unwrap().get(run).cloned().unwrap_or_default();
            for (name, data) in files {
                let safe: String = name.chars().map(|c| if c == '/' || c == '\\' { '_' } else { c }).collect();
                let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                request(&vm.vsock, &json!({ "op": "write", "path": format!("/workspace/inputs/{safe}"), "data_b64": b64 }), FILE_REQUEST_TIMEOUT).await?;
            }
        }
        vms.insert(run.to_string(), vm.clone());
        Ok(vm)
    }

    pub async fn exec(&self, run: &str, command: &str, timeout_s: u64) -> Result<Value> {
        let vm = self.vm(run).await?;
        request(&vm.vsock, &json!({ "op": "exec", "command": command, "timeout_s": timeout_s }), exec_reply_timeout(timeout_s)).await
    }

    pub async fn write(&self, run: &str, path: &str, data: &[u8]) -> Result<Value> {
        let vm = self.vm(run).await?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(data);
        request(&vm.vsock, &json!({ "op": "write", "path": path, "data_b64": b64 }), FILE_REQUEST_TIMEOUT).await
    }

    pub async fn read(&self, run: &str, path: &str) -> Result<Vec<u8>> {
        let vm = self.vm(run).await?;
        let v = request(&vm.vsock, &json!({ "op": "read", "path": path }), FILE_REQUEST_TIMEOUT).await?;
        if let Some(e) = v["error"].as_str() {
            return Err(anyhow!("{e}"));
        }
        Ok(base64::engine::general_purpose::STANDARD.decode(v["data_b64"].as_str().unwrap_or_default())?)
    }

    /// Run a contract check's command in a VM of its own: the image, plus a
    /// copy of the run's workspace that is thrown away afterwards. The run's
    /// VM is stopped first so its disk is consistent; its next use boots it
    /// again on the same disk.
    pub async fn exec_check(&self, run: &str, command: &str, timeout_s: u64) -> Result<Value> {
        self.in_check_vm(run, json!({ "op": "exec", "command": command, "timeout_s": timeout_s }), exec_reply_timeout(timeout_s)).await
    }

    /// Read a file as a contract check sees it, in a check VM.
    pub async fn read_check(&self, run: &str, path: &str) -> Result<Vec<u8>> {
        let v = self.in_check_vm(run, json!({ "op": "read", "path": path }), FILE_REQUEST_TIMEOUT).await?;
        if let Some(e) = v["error"].as_str() {
            return Err(anyhow!("{e}"));
        }
        Ok(base64::engine::general_purpose::STANDARD.decode(v["data_b64"].as_str().unwrap_or_default())?)
    }

    /// Resolve the tools each check command runs against the image, in a
    /// check VM whose workspace is empty (`check_tools.py` in the image): one
    /// list of problems per command.
    /// Resolve each check command's tools against the image: what is wrong
    /// with it, and why its verdict rests on workspace code (empty when the
    /// image's tools alone decide it).
    pub async fn resolve_check_tools(&self, commands: &[String]) -> Result<Vec<Resolved>> {
        let dir = self.scratch_dir();
        let result = async {
            let vm = self.launch_in(&dir, true).await?;
            let arg = base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(commands)?);
            let v = request(
                &vm.vsock,
                &json!({ "op": "exec", "command": format!("python3 -I /usr/local/lib/magnus/check_tools.py {arg}"), "timeout_s": CHECK_TOOLS_TIMEOUT_SECONDS }),
                exec_reply_timeout(CHECK_TOOLS_TIMEOUT_SECONDS),
            )
            .await?;
            anyhow::ensure!(
                v["exit_code"] == json!(0),
                "the image's resolver failed: {}",
                v["stderr"].as_str().unwrap_or_default().chars().rev().take(400).collect::<String>().chars().rev().collect::<String>()
            );
            let found: Vec<Resolved> = serde_json::from_str(v["stdout"].as_str().unwrap_or_default().trim())?;
            anyhow::ensure!(found.len() == commands.len(), "the image's resolver answered for {} of {} commands", found.len(), commands.len());
            Ok(found)
        }
        .await;
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    async fn in_check_vm(&self, run: &str, req: Value, timeout: Duration) -> Result<Value> {
        self.stop(run).await;
        let dir = self.scratch_dir();
        std::fs::create_dir_all(&dir)?;
        let result = async {
            let source = self.dir(run).join("workspace.ext4");
            if source.exists() {
                let out = Command::new("cp").args(["--sparse=always", "--reflink=auto"]).arg(&source).arg(dir.join("workspace.ext4")).output().context("copy the workspace")?;
                anyhow::ensure!(out.status.success(), "copying the workspace failed: {}", String::from_utf8_lossy(&out.stderr));
            }
            let vm = self.launch_in(&dir, true).await?;
            request(&vm.vsock, &req, timeout).await
        }
        .await;
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    /// How many microVMs are running now.
    pub async fn running(&self) -> usize {
        self.vms.lock().await.len()
    }

    /// Whether a run's VM is stopped with its disk kept, to go on from where
    /// it was (a suspended run's, a kill switch's).
    pub fn kept(&self, run: &str) -> bool {
        self.dir(run).join("workspace.ext4").exists()
    }

    /// Stop the run's VM and keep its disk (suspend).
    pub async fn stop(&self, run: &str) {
        if let Some(vm) = self.vms.lock().await.remove(run) {
            let _ = request(&vm.vsock, &json!({ "op": "sync" }), SYNC_TIMEOUT).await;
            drop(vm);
        }
    }

    /// Stop the VM and delete the workspace (the run ended).
    pub async fn destroy(&self, run: &str) {
        self.stop(run).await;
        let _ = std::fs::remove_dir_all(self.dir(run));
    }
}

async fn request(vsock: &Path, req: &Value, timeout: Duration) -> Result<Value> {
    tokio::time::timeout(timeout, async {
        let s = UnixStream::connect(vsock).await?;
        let (r, mut w) = s.into_split();
        w.write_all(format!("CONNECT {GUEST_PORT}\n").as_bytes()).await?;
        let mut r = BufReader::new(r);
        let mut ack = String::new();
        r.read_line(&mut ack).await?;
        anyhow::ensure!(ack.starts_with("OK"), "guest agent not listening");
        w.write_all(format!("{req}\n").as_bytes()).await?;
        let mut line = String::new();
        r.read_line(&mut line).await?;
        anyhow::ensure!(!line.is_empty(), "guest agent closed the connection");
        Ok(serde_json::from_str(&line)?)
    })
    .await
    .map_err(|_| anyhow!("the sandbox did not answer in time"))?
}

#[cfg(test)]
mod exec_timeout_tests {
    use super::*;

    #[test]
    fn a_huge_command_timeout_does_not_wrap_to_a_short_wait() {
        assert_eq!(exec_reply_timeout(u64::MAX), Duration::from_secs(u64::MAX));
        assert_eq!(exec_reply_timeout(30), Duration::from_secs(30 + EXEC_REPLY_GRACE_SECONDS));
    }
}
