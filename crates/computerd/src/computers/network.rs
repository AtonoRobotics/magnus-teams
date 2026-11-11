//! A computer's network: a namespace holding one TAP device and nothing else.
//! There is no route out of it to drop or forget. The only way off the TAP is
//! the forwarder, which carries bytes to Magnus's egress proxy over a Unix
//! socket, so a guest with root can rewrite anything and still has one exit.

use super::*;
use crate::forward::{GATEWAY, GUEST_NETMASK_BITS};

const IP_TOOL: &str = "/usr/sbin/ip";
pub(super) const NETNS_DIR: &str = "/var/run/netns";
/// Every computer's namespace is named this and its id.
pub(super) const NETNS_PREFIX: &str = "magnus-";
pub const TAP: &str = "tap0";
pub const GUEST_MAC: &str = "06:00:0a:c8:00:02";
pub const EGRESS_DIR: &str = "egress";
pub const PROXY_SOCKET: &str = "proxy.sock";

fn ip(args: &[&str]) -> Result<()> {
    let out = Command::new(IP_TOOL).env_clear().args(args).output().context("run ip")?;
    ensure!(out.status.success(), "ip {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    Ok(())
}

impl Computers {
    fn netns_name(&self, id: &ComputerId) -> String {
        format!("{NETNS_PREFIX}{}", id.as_str())
    }

    pub(super) fn netns_path(&self, id: &ComputerId) -> PathBuf {
        self.netns_dir.join(self.netns_name(id))
    }

    pub fn proxy_socket(&self) -> PathBuf {
        self.state(EGRESS_DIR).join(PROXY_SOCKET)
    }

    /// Made new for every boot, so nothing of an earlier one is left in it.
    pub(super) fn prepare_network(&self, id: &ComputerId, uid: u32) -> Result<()> {
        let name = self.netns_name(id);
        self.remove_network(id);
        ip(&["netns", "add", &name])?;
        let owner = uid.to_string();
        ip(&["-n", &name, "tuntap", "add", "dev", TAP, "mode", "tap", "user", &owner, "group", &owner])?;
        ip(&["-n", &name, "addr", "add", &format!("{GATEWAY}/{GUEST_NETMASK_BITS}"), "dev", TAP])?;
        ip(&["-n", &name, "link", "set", TAP, "up"])
    }

    /// Goes with the VM, and is made again at its next boot. What `ip netns
    /// del` does: the namespace's mount goes, then its name. A forwarder
    /// still in it keeps the namespace itself only until it sees its VM gone.
    pub(super) fn remove_network(&self, id: &ComputerId) {
        remove_netns(&self.netns_path(id));
    }

    /// Started once the VM is up. It stops by itself when the VM has gone.
    pub(super) fn start_forwarder(&self, id: &ComputerId, uid: u32) -> Result<()> {
        let pids = std::fs::read_to_string(self.cgroup(id).join("cgroup.procs")).context("find the VM's process")?.split_whitespace().collect::<Vec<_>>().join(",");
        let mut child = Command::new(std::env::current_exe().context("find this program")?)
            .env_clear()
            .arg("forward")
            .arg(self.netns_path(id))
            .arg(uid.to_string())
            .arg(self.cfg.socket_gid.to_string())
            .arg(self.proxy_socket())
            .arg(pids)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("start the forwarder")?;
        // Reaped here so it is no zombie, and left alone by a computerd restart.
        std::thread::spawn(move || child.wait());
        Ok(())
    }
}

/// A namespace's name, gone. Not mounted, or not root as in a test: the name
/// still goes.
pub(super) fn remove_netns(path: &Path) {
    if let Ok(c) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) {
        unsafe { libc::umount2(c.as_ptr(), libc::MNT_DETACH) };
    }
    let _ = std::fs::remove_file(path);
}
