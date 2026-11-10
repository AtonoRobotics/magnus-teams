//! A real `magnus-computerd` for one test, with its own state directory. Needs
//! passwordless sudo, KVM, the guest images, and a test process in the
//! `magnus` group, the way the appliance runs.

use crate::appliance::{bin, sandbox_assets};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const SOCKET_WAIT: Duration = Duration::from_secs(10);
const CGROUPS: &str = "/sys/fs/cgroup/magnus-computers";
/// computerd's default.
const PURGE_DAYS_WHEN_UNSET: u64 = 30;

pub struct Computerd {
    child: Child,
    state: PathBuf,
    pub socket: PathBuf,
}

fn sudo_sh(script: &str) {
    let out = Command::new("sudo").args(["-n", "sh", "-c", script]).output().expect("run sudo");
    assert!(out.status.success(), "{script}: {}", String::from_utf8_lossy(&out.stderr));
}

impl Computerd {
    pub fn start() -> Computerd {
        let (kernel, rootfs) = sandbox_assets();
        let name = uuid::Uuid::new_v4().simple().to_string();
        let state = PathBuf::from(format!("/var/lib/magnus-invariant-{}", &name[..8]));
        let socket = state.join("s.sock");
        let group = Command::new("getent").args(["group", "magnus"]).output().expect("getent");
        let gid = String::from_utf8_lossy(&group.stdout).split(':').nth(2).expect("the magnus group exists").to_string();
        let config = format!(
            "state_dir = \"{s}\"\nsocket = \"{k}\"\nsocket_gid = {gid}\njailer = \"/usr/local/bin/jailer\"\nfirecracker = \"/usr/local/bin/firecracker\"\nfirst_uid = 220000\nmax_computers = 8\nmax_live = 4\n[vm]\nvcpus = 1\nmemory_mib = 1024\ncpu_percent = 100\npids_max = 128\nvolume_gib = 1\noverlay_gib = 1\n",
            s = state.display(),
            k = socket.display(),
        );
        sudo_sh(&format!(
            "umask 022; mkdir -p {s}/images {s}/computers {s}/jail; cp -L --sparse=always {k} {s}/images/vmlinux; cp -L --sparse=always {r} {s}/images/rootfs.ext4; echo base-one > {s}/images/base.id; chmod 644 {s}/images/base.id; cat > {s}/config.toml <<'EOF'\n{config}EOF\n",
            s = state.display(),
            k = kernel.display(),
            r = rootfs.display(),
        ));
        // Started by path, so the test can stop exactly this one.
        let child = Command::new("sudo")
            .args(["-n"])
            .arg(bin("magnus-computerd"))
            .args(["serve", "--config"])
            .arg(state.join("config.toml"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .expect("start computerd");
        let started = Instant::now();
        while UnixStream::connect(&socket).is_err() {
            assert!(started.elapsed() < SOCKET_WAIT, "computerd did not listen: is this test in the magnus group?");
            std::thread::sleep(Duration::from_millis(50));
        }
        Computerd { child, state, socket }
    }

    /// The processes of a computer, from the kernel.
    pub fn procs(&self, id: &str) -> Vec<String> {
        // Root-only, and a read that fails must not look like no processes.
        let file = Path::new(CGROUPS).join(id).join("cgroup.procs");
        let out = Command::new("sudo").args(["-n", "sh", "-c", &format!("[ ! -e {f} ] || cat {f}", f = file.display())]).output().expect("run sudo");
        assert!(out.status.success(), "could not read {}: {}", file.display(), String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).split_whitespace().map(String::from).collect()
    }

    pub fn volume_exists(&self, id: &str) -> bool {
        Command::new("sudo").args(["-n", "test", "-f"]).arg(self.state.join("computers").join(id).join("volume.ext4")).status().is_ok_and(|s| s.success())
    }

    fn computer_dir(&self, id: &str) -> PathBuf {
        self.state.join("computers").join(id)
    }

    fn backups_dir(&self, id: &str) -> PathBuf {
        self.state.join("backups").join(id)
    }

    /// One request to computerd itself.
    pub fn ask(&self, req: Value) -> Value {
        let mut conn = UnixStream::connect(&self.socket).expect("reach computerd");
        writeln!(conn, "{req}").expect("ask computerd");
        let mut line = String::new();
        BufReader::new(conn).read_line(&mut line).expect("hear computerd");
        let reply: Value = serde_json::from_str(&line).expect("computerd's reply");
        assert_eq!(reply["ok"], true, "{req}: {reply}");
        reply
    }

    /// What a command in the computer printed.
    pub fn exec(&self, id: &str, command: &str) -> String {
        let mut conn = UnixStream::connect(&self.socket).expect("reach computerd");
        writeln!(conn, "{}", json!({ "op": "connect", "id": id })).expect("connect");
        let mut from = BufReader::new(conn.try_clone().expect("clone"));
        let mut ack = String::new();
        from.read_line(&mut ack).expect("hear the relay");
        assert!(ack.contains("true"), "{ack}");
        writeln!(conn, "{}", json!({ "op": "exec", "command": command, "timeout_s": 300 })).expect("send");
        let mut line = String::new();
        from.read_line(&mut line).expect("hear the guest");
        let reply: Value = serde_json::from_str(&line).expect("the guest's reply");
        reply["stdout"].as_str().unwrap_or_default().to_string()
    }

    /// Whether a sealed volume reads as holding `secret`, in any encoding.
    pub fn sealed_holds(&self, id: &str, secret: &str) -> bool {
        self.file_holds(&self.state.join("computers").join(id).join("volume.sealed"), secret)
    }
    /// computerd's reply, whether it agreed or not.
    pub fn try_ask(&self, req: Value) -> Value {
        let mut conn = UnixStream::connect(&self.socket).expect("reach computerd");
        writeln!(conn, "{req}").expect("ask computerd");
        let mut line = String::new();
        BufReader::new(conn).read_line(&mut line).expect("hear computerd");
        serde_json::from_str(&line).expect("computerd's reply")
    }
    /// What the guest answers to one request, whatever it is.
    pub fn guest(&self, id: &str, req: Value) -> Value {
        let mut conn = UnixStream::connect(&self.socket).expect("reach computerd");
        writeln!(conn, "{}", json!({ "op": "connect", "id": id })).expect("connect");
        let mut from = BufReader::new(conn.try_clone().expect("clone"));
        let mut ack = String::new();
        from.read_line(&mut ack).expect("hear the relay");
        assert!(ack.contains("true"), "{ack}");
        writeln!(conn, "{req}").expect("send");
        let mut line = String::new();
        from.read_line(&mut line).expect("hear the guest");
        serde_json::from_str(&line).expect("the guest's reply")
    }
    /// As root in the guest, with a shell. Returns what it printed.
    pub fn root_shell(&self, id: &str, script: &str) -> String {
        self.exec(id, script)
    }

    /// Everything that can be rebuilt, gone from the disk.
    pub fn remove_overlay_and_snapshot(&self, id: &str) {
        let d = self.computer_dir(id);
        sudo_sh(&format!("rm -rf {d}/overlay.ext4 {d}/snapshot", d = d.display()));
    }

    pub fn remove_volume(&self, id: &str) {
        sudo_sh(&format!("rm -f {}/volume.ext4", self.computer_dir(id).display()));
    }

    pub fn snapshot_aside(&self, id: &str) {
        let d = self.computer_dir(id);
        sudo_sh(&format!("cp -a {d}/snapshot {d}/snapshot.aside", d = d.display()));
    }

    pub fn snapshot_back(&self, id: &str) {
        let d = self.computer_dir(id);
        sudo_sh(&format!("rm -rf {d}/snapshot; cp -a {d}/snapshot.aside {d}/snapshot", d = d.display()));
    }

    /// Whether `file` holds `secret` in any encoding `scan` knows.
    pub fn file_holds(&self, file: &Path, secret: &str) -> bool {
        crate::scan::encodings(secret).iter().any(|needle| {
            let status = Command::new("sudo").args(["-n", "grep", "-qaF", "--"]).arg(String::from_utf8_lossy(needle).as_ref()).arg(file).status().expect("run grep");
            status.code() == Some(0)
        })
    }

    fn opened(&self, name: &str) -> PathBuf {
        self.state.join(format!("opened-{name}"))
    }

    fn run_root(&self, args: &[&std::ffi::OsStr]) {
        let out = Command::new("sudo").arg("-n").arg(bin("magnus-computerd")).args(args).output().expect("run computerd's tool");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    /// `secret` in the stored snapshot files as they sit on disk, then in
    /// their plaintext. The two together show the scan read real data.
    pub fn snapshot_holds(&self, id: &str, secrets: &[&str]) -> Vec<(bool, bool)> {
        let dir = self.computer_dir(id).join("snapshot");
        let mut found = vec![(false, false); secrets.len()];
        for name in ["snapshot.mem", "snapshot.state"] {
            let file = dir.join(name);
            // Opening the whole of memory is slow in a debug build, so once.
            let opened = self.opened(name);
            self.run_root(&["open-snapshot".as_ref(), dir.join("key").as_os_str(), file.as_os_str(), opened.as_os_str()]);
            for (slot, secret) in found.iter_mut().zip(secrets) {
                slot.0 |= self.file_holds(&file, secret);
                slot.1 |= self.file_holds(&opened, secret);
            }
            sudo_sh(&format!("rm -f {}", opened.display()));
        }
        found
    }

    /// What the volume costs the host's disk, rather than its apparent size.
    pub fn volume_allocated_bytes(&self, id: &str) -> u64 {
        let out = Command::new("sudo").args(["-n", "du", "--block-size=1"]).arg(self.computer_dir(id).join("volume.ext4")).output().expect("run du");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).split_whitespace().next().and_then(|n| n.parse().ok()).expect("a size from du")
    }

    pub fn volume_holds(&self, id: &str, secret: &str) -> bool {
        self.file_holds(&self.computer_dir(id).join("volume.ext4"), secret)
    }

    /// Every chunk stored for the computer, decrypted.
    pub fn backups_hold(&self, id: &str, secret: &str) -> bool {
        let dir = self.backups_dir(id);
        let opened = self.opened("backups");
        self.run_root(&["open-backups".as_ref(), dir.as_os_str(), opened.as_os_str()]);
        let found = self.file_holds(&opened, secret);
        sudo_sh(&format!("rm -f {}", opened.display()));
        found
    }

    /// The times of the backups kept for the computer, oldest first.
    pub fn backups(&self, id: &str) -> Vec<u64> {
        let out = Command::new("sudo").args(["-n", "sh", "-c", &format!("ls {} 2>/dev/null || true", self.backups_dir(id).display())]).output().expect("list backups");
        let mut at: Vec<u64> = String::from_utf8_lossy(&out.stdout).split_whitespace().filter_map(|n| n.strip_suffix(".list")?.parse().ok()).collect();
        at.sort();
        at
    }

    pub fn has_snapshot(&self, id: &str) -> bool {
        Command::new("sudo").args(["-n", "test", "-d"]).arg(self.computer_dir(id).join("snapshot")).status().is_ok_and(|s| s.success())
    }

    pub fn purge_days(&self) -> u64 {
        let out = Command::new("sudo").args(["-n", "cat"]).arg(self.state.join("config.toml")).output().expect("read the config");
        let cfg: toml::Value = toml::from_str(&String::from_utf8_lossy(&out.stdout)).expect("the config");
        cfg.get("purge_days").and_then(toml::Value::as_integer).map_or(PURGE_DAYS_WHEN_UNSET, |d| d as u64)
    }

    /// Runs the purge as if the clock read `unix_seconds`.
    pub fn purge_at(&self, unix_seconds: u64) {
        let (config, at) = (self.state.join("config.toml"), unix_seconds.to_string());
        self.run_root(&["purge".as_ref(), config.as_os_str(), at.as_ref()]);
    }

    fn ids(&self) -> Vec<String> {
        let out = Command::new("sudo").args(["-n", "ls"]).arg(self.state.join("computers")).output().expect("list computers");
        String::from_utf8_lossy(&out.stdout).split_whitespace().map(String::from).collect()
    }

    fn stop(&self, id: &str) {
        let Ok(mut conn) = UnixStream::connect(&self.socket) else { return };
        if writeln!(conn, "{{\"op\":\"stop\",\"id\":\"{id}\"}}").is_ok() {
            let mut line = String::new();
            let _ = BufReader::new(conn).read_line(&mut line);
        }
    }
}

impl Drop for Computerd {
    fn drop(&mut self) {
        // Only this test's computers: others on the machine are left alone.
        for id in self.ids() {
            self.stop(&id);
        }
        let _ = Command::new("sudo").args(["-n", "pkill", "-f"]).arg(self.state.join("config.toml")).status();
        let _ = self.child.wait();
        let _ = Command::new("sudo").args(["-n", "rm", "-rf"]).arg(&self.state).status();
    }
}
