//! Real VMs, checked from outside. Needs root, KVM and a directory of images
//! (`MAGNUS_COMPUTERD_IMAGES`, holding `vmlinux` and `rootfs.ext4`). Run the
//! built test binary with sudo. It stops every computer on the machine.

#[cfg(test)]
mod real_vms {
    use serde_json::{json, Value};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::net::UnixStream;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    const STATE: &str = "/var/lib/magnus-computerd-test";
    const CGROUPS: &str = "/sys/fs/cgroup/magnus-computers";
    const FIRST_UID: u32 = 210_000;
    const BOOT_WAIT: Duration = Duration::from_secs(30);
    const SOCKET_WAIT: Duration = Duration::from_secs(5);
    const MEMORY_MIB: u64 = 256;
    const OVERHEAD_MIB: u64 = 256;

    fn program() -> Command {
        Command::new(env!("CARGO_BIN_EXE_magnus-computerd"))
    }

    fn config_path() -> PathBuf {
        Path::new(STATE).join("config.toml")
    }

    fn socket() -> PathBuf {
        Path::new(STATE).join("s.sock")
    }

    /// Killed even when a check fails, or it would hold the test's output open.
    struct Daemon(Child);
    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn start_daemon() -> Daemon {
        let _ = std::fs::remove_file(socket());
        let mut child = program().args(["serve", "--config"]).arg(config_path()).stdin(Stdio::null()).spawn().unwrap();
        let started = Instant::now();
        while UnixStream::connect(socket()).is_err() {
            if let Some(status) = child.try_wait().unwrap() {
                panic!("computerd exited at start: {status}");
            }
            assert!(started.elapsed() < SOCKET_WAIT, "computerd did not listen");
            std::thread::sleep(Duration::from_millis(20));
        }
        Daemon(child)
    }

    fn ask(req: Value) -> Value {
        let mut conn = UnixStream::connect(socket()).unwrap();
        writeln!(conn, "{req}").unwrap();
        let mut line = String::new();
        BufReader::new(conn).read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }

    fn ok(req: Value) -> Value {
        let r = ask(req.clone());
        assert_eq!(r["ok"], true, "{req} failed: {r}");
        r
    }

    fn refused(req: Value, needle: &str) {
        let r = ask(req.clone());
        assert_eq!(r["ok"], false, "{req} was allowed: {r}");
        assert!(r["error"].as_str().unwrap().contains(needle), "{req}: {r}");
    }

    fn state(id: &str) -> String {
        ok(json!({"op":"status","id":id}))["state"].as_str().unwrap().to_string()
    }

    fn cgroup(id: &str) -> PathBuf {
        Path::new(CGROUPS).join(id)
    }

    fn procs(id: &str) -> Vec<u32> {
        std::fs::read_to_string(cgroup(id).join("cgroup.procs")).unwrap_or_default().split_whitespace().map(|p| p.parse().unwrap()).collect()
    }

    /// Through computerd, the way Magnus reaches a guest.
    fn try_guest(id: &str, req: &Value) -> std::io::Result<Value> {
        let mut s = UnixStream::connect(socket())?;
        s.set_read_timeout(Some(Duration::from_secs(5)))?;
        writeln!(s, "{}", json!({"op":"connect","id":id}))?;
        let mut r = BufReader::new(s.try_clone()?);
        let mut ack = String::new();
        r.read_line(&mut ack)?;
        let ack: Value = serde_json::from_str(&ack)?;
        if ack["ok"] != true {
            return Err(std::io::Error::other(ack.to_string()));
        }
        writeln!(s, "{req}")?;
        let mut line = String::new();
        r.read_line(&mut line)?;
        Ok(serde_json::from_str(&line)?)
    }

    fn guest(id: &str, req: Value) -> Value {
        try_guest(id, &req).unwrap_or_else(|e| panic!("guest {id}: {e}"))
    }

    fn wait_for_boot(id: &str) {
        let started = Instant::now();
        while try_guest(id, &json!({"op":"ping"})).is_err() {
            assert!(started.elapsed() < BOOT_WAIT, "computer {id} did not boot; procs {:?}", procs(id));
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn read_note(id: &str) -> String {
        guest(id, json!({"op":"exec","command":"cat /workspace/note","timeout_s":10}))["stdout"].as_str().unwrap_or_default().to_string()
    }

    fn uid_of(pid: u32) -> u32 {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
        status.lines().find(|l| l.starts_with("Uid:")).unwrap().split_whitespace().nth(1).unwrap().parse().unwrap()
    }

    fn cli(verb: &str) -> String {
        let out = program().arg(verb).arg("--config").arg(config_path()).output().unwrap();
        assert!(out.status.success(), "{verb}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    fn setup() {
        let images = PathBuf::from(std::env::var("MAGNUS_COMPUTERD_IMAGES").expect("set MAGNUS_COMPUTERD_IMAGES"));
        assert_eq!(unsafe { libc::geteuid() }, 0, "run this test as root");
        let _ = std::fs::remove_dir_all(STATE);
        std::fs::create_dir_all(Path::new(STATE).join("images")).unwrap();
        std::fs::create_dir_all(Path::new(STATE).join("computers")).unwrap();
        for name in ["vmlinux", "rootfs.ext4"] {
            std::fs::copy(images.join(name), Path::new(STATE).join("images").join(name)).unwrap();
        }
        std::fs::write(
            config_path(),
            format!(
                r#"
    state_dir = "{STATE}"
    socket = "{STATE}/s.sock"
    socket_gid = 0
    jailer = "/usr/local/bin/jailer"
    firecracker = "/usr/local/bin/firecracker"
    first_uid = {FIRST_UID}
    max_computers = 4
    max_live = 2
    [vm]
    vcpus = 1
    memory_mib = {MEMORY_MIB}
    cpu_percent = 100
    pids_max = 128
    volume_gib = 1
    overlay_gib = 1
    "#
            ),
        )
        .unwrap();
    }

    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = program().arg("kill-all").arg("--config").arg(config_path()).output();
            let _ = std::fs::remove_dir_all(STATE);
        }
    }

    #[test]
    fn computers_are_jailed_outlive_the_service_and_die_with_the_owners_kill() {
        setup();
        let _cleanup = Cleanup;
        let mut daemon = start_daemon();

        // Nothing but a valid id reaches a path.
        refused(json!({"op":"create","id":"../../etc"}), "lowercase");
        refused(json!({"op":"create","id":"a","path":"/etc/shadow"}), "bad request");
        refused(json!({"op":"start","id":"a","memory_mib":99999}), "bad request");

        let a = ok(json!({"op":"create","id":"a"}))["uid"].as_u64().unwrap() as u32;
        let b = ok(json!({"op":"create","id":"b"}))["uid"].as_u64().unwrap() as u32;
        ok(json!({"op":"create","id":"c"}));
        assert_ne!(a, b, "two computers share a uid");
        ok(json!({"op":"start","id":"a"}));
        ok(json!({"op":"start","id":"b"}));
        refused(json!({"op":"start","id":"c"}), "as many computers");
        wait_for_boot("a");
        wait_for_boot("b");

        // Each VM: its own unprivileged uid, in a chroot, with the config's caps.
        for (id, uid) in [("a", a), ("b", b)] {
            let pids = procs(id);
            assert!(!pids.is_empty(), "no process for {id}");
            for pid in &pids {
                assert_eq!(uid_of(*pid), uid, "a process of {id} runs as the wrong user");
                let inside = format!("/proc/{pid}/root");
                assert!(Path::new(&inside).join("volume.ext4").exists(), "{id} has no volume in its jail");
                for host_only in ["home", "usr", "etc", "var"] {
                    assert!(!Path::new(&inside).join(host_only).exists(), "{id} can see /{host_only}");
                }
            }
            let read = |f: &str| std::fs::read_to_string(cgroup(id).join(f)).unwrap().trim().to_string();
            assert_eq!(read("memory.max"), ((MEMORY_MIB + OVERHEAD_MIB) << 20).to_string());
            assert_eq!(read("cpu.max"), "100000 100000");
            assert_eq!(read("pids.max"), "128");
            let volume = std::fs::metadata(Path::new(STATE).join("computers").join(id).join("volume.ext4")).unwrap();
            assert_eq!((volume.uid(), volume.mode() & 0o777), (uid, 0o600), "{id}'s volume is not private to it");
        }

        guest("a", json!({"op":"write","path":"/workspace/note","data_b64":"a2VlcCBtZQ=="}));
        guest("a", json!({"op":"sync"}));
        assert!(read_note("a").contains("keep me"));

        // A VM's user owns its jail. A link planted there must not make root
        // reach anything outside it: the service's own socket stands in.
        let planted = Path::new(STATE).join("jail/firecracker/b/root/v.sock");
        std::fs::remove_file(&planted).unwrap();
        std::os::unix::fs::symlink(socket(), &planted).unwrap();
        let reached = try_guest("b", &json!({"op":"list"}));
        assert!(reached.is_err(), "a planted link reached the service's socket: {reached:?}");

        // The VMs do not depend on the service.
        drop(daemon);
        assert!(!procs("a").is_empty(), "a computer died with the service");
        daemon = start_daemon();
        assert_eq!(state("a"), "running");
        wait_for_boot("a");

        // Pause and resume.
        ok(json!({"op":"pause","id":"a"}));
        assert_eq!(state("a"), "paused");
        ok(json!({"op":"resume","id":"a"}));
        assert_eq!(state("a"), "running");
        wait_for_boot("a");

        // Stopping leaves no process and keeps the work.
        ok(json!({"op":"stop","id":"a"}));
        assert!(procs("a").is_empty(), "a process survived the stop");
        assert_eq!(state("a"), "stopped");
        ok(json!({"op":"start","id":"a"}));
        wait_for_boot("a");
        assert!(read_note("a").contains("keep me"), "the stop lost the volume");

        // A stopped computer's files, through a helper VM with only the volume.
        ok(json!({"op":"stop","id":"b"}));
        ok(json!({"op":"start_helper","id":"c"}));
        wait_for_boot("c");
        guest("c", json!({"op":"write","path":"/workspace/note","data_b64":"aGVscGVy"}));
        guest("c", json!({"op":"sync"}));
        ok(json!({"op":"stop","id":"c"}));
        assert!(!Path::new(STATE).join("computers/c/overlay.ext4").exists(), "a helper VM made an overlay");
        ok(json!({"op":"start","id":"b"}));
        wait_for_boot("b");

        // The owner's kill needs neither Magnus nor the service, and sticks.
        drop(daemon);
        let said = cli("kill-all");
        assert!(said.contains("killed 2"), "{said}");
        assert!(procs("a").is_empty() && procs("b").is_empty(), "a computer survived kill-all");
        assert!(!cgroup("a").exists());
        daemon = start_daemon();
        refused(json!({"op":"start","id":"a"}), "halted");
        cli("release");
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(state("a"), "stopped", "release resumed a computer");

        // Destroying keeps the volume unless asked.
        ok(json!({"op":"start","id":"a"}));
        wait_for_boot("a");
        ok(json!({"op":"destroy","id":"a"}));
        assert!(!Path::new(STATE).join("computers/a/overlay.ext4").exists(), "destroy kept the overlay");
        ok(json!({"op":"start","id":"a"}));
        wait_for_boot("a");
        assert!(read_note("a").contains("keep me"), "destroy lost the volume");
        ok(json!({"op":"destroy","id":"a","delete_volume":true}));
        assert!(!Path::new(STATE).join("computers/a").exists(), "the computer was not deleted");
        refused(json!({"op":"status","id":"a"}), "no computer");
        drop(daemon);
    }
}
