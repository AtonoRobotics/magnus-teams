//! What a VM leaves behind once it is gone, cleared when computerd starts.
//!
//! Computer VMs outlive computerd: the jailer daemonizes them into their own
//! cgroups, and a computerd that restarts reads which run from those cgroups,
//! so it knows every live VM and counts it against the machine's capacity
//! (`decisions/2026-11-11-computerd-restarts-keep-the-vms.md`). What it does
//! not know is what belonged to a VM that went while no computerd watched: a
//! network namespace, and a jail that may still hold a memory snapshot in
//! the clear if a hibernation was cut off. Those go here.

use super::*;
use network::{remove_netns, NETNS_PREFIX};

impl Computers {
    /// Every computer namespace whose VM is not running.
    pub fn sweep_networks(&self) -> usize {
        let Ok(dir) = std::fs::read_dir(&self.netns_dir) else { return 0 };
        let mut removed = 0;
        for e in dir.flatten() {
            let name = e.file_name();
            let Some(id) = name.to_str().and_then(|n| n.strip_prefix(NETNS_PREFIX)).and_then(|n| ComputerId::parse(n).ok()) else { continue };
            if !self.is_live(&id) {
                remove_netns(&e.path());
                removed += 1;
            }
        }
        removed
    }

    /// Every jail whose VM is not running.
    fn sweep_jails(&self) -> Result<usize> {
        let Ok(dir) = std::fs::read_dir(self.state("jail").join(FIRECRACKER_NAME)) else { return Ok(0) };
        let mut removed = 0;
        for e in dir.flatten() {
            let Some(id) = e.file_name().to_str().and_then(|n| ComputerId::parse(n).ok()) else { continue };
            if !self.is_live(&id) {
                remove_if_there(&e.path())?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Run once as computerd starts, before it takes a request.
    pub fn sweep(&self) -> Result<()> {
        let _lock = self.lock()?;
        let networks = self.sweep_networks();
        let jails = self.sweep_jails()?;
        if networks + jails > 0 {
            eprintln!("magnus-computerd: cleared what {networks} network namespaces and {jails} jails of stopped computers left behind");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn computers(state: &Path) -> Computers {
        let mut cfg = super::super::tests::config();
        cfg.state_dir = state.into();
        let mut c = Computers::new(cfg);
        c.cgroups = state.join("cgroups");
        c.netns_dir = state.join("netns");
        std::fs::create_dir_all(&c.netns_dir).unwrap();
        c
    }

    /// `id` runs, as its cgroup says.
    fn running(c: &Computers, id: &str) {
        let cg = c.cgroups.join(id);
        std::fs::create_dir_all(&cg).unwrap();
        std::fs::write(cg.join("cgroup.procs"), "4242\n").unwrap();
    }

    #[test]
    fn a_stopped_computers_namespace_and_jail_go_and_a_running_ones_stay() {
        let state = tempfile::tempdir().unwrap();
        let c = computers(state.path());
        for name in ["magnus-abc", "magnus-abd", "someone-elses", "magnus-NOT_AN_ID"] {
            std::fs::write(c.netns_dir.join(name), b"").unwrap();
        }
        for id in ["abc", "abd"] {
            std::fs::create_dir_all(c.jail_root(&ComputerId::parse(id).unwrap())).unwrap();
            std::fs::write(c.jail_root(&ComputerId::parse(id).unwrap()).join("snapshot.mem"), b"memory").unwrap();
        }
        running(&c, "abd");
        c.sweep().unwrap();
        assert!(!c.netns_dir.join("magnus-abc").exists(), "a stopped computer kept its namespace");
        assert!(!state.path().join("jail/firecracker/abc").exists(), "a stopped computer kept its jail");
        assert!(c.netns_dir.join("magnus-abd").exists(), "a running computer lost its network");
        assert!(state.path().join("jail/firecracker/abd/root/snapshot.mem").exists(), "a running computer lost its jail");
        assert!(c.netns_dir.join("someone-elses").exists() && c.netns_dir.join("magnus-NOT_AN_ID").exists(), "a namespace not ours went");
    }

    #[test]
    fn stopping_a_computer_removes_its_namespace() {
        let state = tempfile::tempdir().unwrap();
        let c = computers(state.path());
        let id = ComputerId::parse("abc").unwrap();
        std::fs::write(c.netns_path(&id), b"").unwrap();
        c.stop(&id).unwrap();
        assert!(!c.netns_path(&id).exists(), "a stopped computer kept its network namespace");
    }
}
