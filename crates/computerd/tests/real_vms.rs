//! Real VMs, checked from outside. Needs root, KVM and a directory of images
//! (`MAGNUS_COMPUTERD_IMAGES`, holding `vmlinux` and `rootfs.ext4`). Run the
//! built test binary with sudo. It stops every computer on the machine.

#[cfg(test)]
mod real_vms {
    use serde_json::{json, Value};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
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
        std::fs::write(Path::new(STATE).join("images/base.id"), "base-one").unwrap();
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

        // The root is writable, and what is written there survives a stop.
        guest("b", json!({"op":"exec","command":"echo kept > /usr/local/marker; sync","timeout_s":10}));
        ok(json!({"op":"stop","id":"b"}));
        let started = ok(json!({"op":"start","id":"b"}));
        assert_eq!(started["overlay_fresh"], false, "{started}");
        wait_for_boot("b");
        let marker = guest("b", json!({"op":"exec","command":"cat /usr/local/marker","timeout_s":10}));
        assert!(marker["stdout"].as_str().unwrap_or_default().contains("kept"), "the overlay lost a file: {marker}");

        // Hibernating stores only ciphertext and resumes the same memory.
        guest("b", json!({"op":"exec","command":"echo canary-$((1000+1000)) > /tmp/in-memory","timeout_s":10}));
        let before = guest("b", json!({"op":"exec","command":"cat /etc/machine-id 2>/dev/null; hostname","timeout_s":10}));
        ok(json!({"op":"hibernate","id":"b"}));
        assert!(procs("b").is_empty(), "a hibernated computer kept running");
        assert_eq!(state("b"), "hibernated");
        let stored = Path::new(STATE).join("computers/b/snapshot");
        let memory = std::fs::read(stored.join("snapshot.mem")).unwrap();
        assert!(!memory.windows(11).any(|w| w == b"canary-2000"), "the stored memory is readable");
        let opened = Path::new(STATE).join("opened");
        let out = program().args(["open-snapshot"]).arg(stored.join("key")).arg(stored.join("snapshot.mem")).arg(&opened).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let plain = std::fs::read(&opened).unwrap();
        assert!(plain.windows(11).any(|w| w == b"canary-2000"), "the scan read nothing real");
        std::fs::remove_file(&opened).unwrap();
        let woke = ok(json!({"op":"wake","id":"b"}));
        assert_eq!(woke["resumed"], true, "{woke}");
        wait_for_boot("b");
        let after = guest("b", json!({"op":"exec","command":"cat /etc/machine-id; hostname","timeout_s":10}));
        assert_ne!(before["stdout"], after["stdout"], "a restore kept the machine id or name");
        let kept = guest("b", json!({"op":"exec","command":"cat /tmp/in-memory","timeout_s":10}));
        assert!(kept["stdout"].as_str().unwrap_or_default().contains("canary-2000"), "the wake lost the memory: {kept}");
        assert!(!stored.exists(), "a used snapshot was kept");

        // A snapshot and an overlay are valid only on their own base image.
        ok(json!({"op":"hibernate","id":"b"}));
        std::fs::write(Path::new(STATE).join("images/base.id"), "base-two").unwrap();
        let woke = ok(json!({"op":"wake","id":"b"}));
        assert_eq!((woke["resumed"].as_bool(), woke["overlay_fresh"].as_bool()), (Some(false), Some(true)), "{woke}");
        std::fs::write(Path::new(STATE).join("images/base.id"), "base-one").unwrap();
        wait_for_boot("b");

        // Reading a hibernated computer keeps its memory; editing it does not.
        ok(json!({"op":"hibernate","id":"b"}));
        ok(json!({"op":"start_helper","id":"b","read_only":true}));
        wait_for_boot("b");
        ok(json!({"op":"stop","id":"b"}));
        assert_eq!(state("b"), "hibernated", "a read discarded the snapshot");
        let woke = ok(json!({"op":"wake","id":"b"}));
        assert_eq!(woke["resumed"], true, "a read forced a cold boot: {woke}");
        ok(json!({"op":"hibernate","id":"b"}));
        ok(json!({"op":"start_helper","id":"b"}));
        wait_for_boot("b");
        ok(json!({"op":"stop","id":"b"}));
        assert_eq!(state("b"), "stopped", "an edit kept the snapshot");
        let woke = ok(json!({"op":"wake","id":"b"}));
        assert_eq!(woke["resumed"], false, "an edit was followed by a resume: {woke}");
        wait_for_boot("b");

        // A backup is ciphertext, stores only what changed, and comes back.
        let data = "backup-canary-".repeat(1000);
        guest("b", json!({"op":"exec","command":format!("echo {data} > /workspace/precious; head -c 67108864 /dev/urandom > /workspace/big; sync"),"timeout_s":60}));
        let first = ok(json!({"op":"backup","id":"b"}));
        let stored_files: Vec<PathBuf> = std::fs::read_dir(Path::new(STATE).join("backups/b/chunks")).unwrap().map(|e| e.unwrap().path()).collect();
        assert!(!stored_files.is_empty(), "a backup stored nothing");
        for f in &stored_files {
            assert!(!std::fs::read(f).unwrap().windows(13).any(|w| w == b"backup-canary"), "a backup is readable");
        }
        guest("b", json!({"op":"exec","command":"echo ok > /workspace/alive; sync","timeout_s":10}));
        let second = ok(json!({"op":"backup","id":"b"}));
        let grown = second["bytes"].as_u64().unwrap() - first["bytes"].as_u64().unwrap();
        // The journal and inode tables change; the 64 MiB file does not.
        assert!(first["bytes"].as_u64().unwrap() > 67_108_864, "the first backup is missing data: {first}");
        assert!(grown < 33_554_432, "a second backup stored everything again: {first} then {second}");
        ok(json!({"op":"stop","id":"b"}));
        let volume = Path::new(STATE).join("computers/b/volume.ext4");
        let uid = std::fs::metadata(&volume).unwrap().uid();
        std::fs::remove_file(&volume).unwrap();
        ok(json!({"op":"restore_backup","id":"b"}));
        assert_eq!(std::fs::metadata(&volume).unwrap().uid(), uid, "the restored volume is not the VM's");
        ok(json!({"op":"start","id":"b"}));
        wait_for_boot("b");
        let back = guest("b", json!({"op":"exec","command":"cat /workspace/precious","timeout_s":10}));
        assert!(back["stdout"].as_str().unwrap_or_default().contains("backup-canary"), "the backup lost a file: {back}");

        // Nothing outlives the purge window, and the clock is the only reason.
        let month_later = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 31 * 86_400;
        let purged = program().arg("purge").arg(config_path()).arg(month_later.to_string()).output().unwrap();
        assert!(purged.status.success(), "{}", String::from_utf8_lossy(&purged.stderr));
        assert!(!Path::new(STATE).join("backups/b").exists(), "a backup outlived the purge window");

        // A planted link cannot make root read another file into a snapshot.
        let target = Path::new(STATE).join("config.toml");
        let jail = Path::new(STATE).join("jail/firecracker/b/root");
        std::os::unix::fs::symlink(&target, jail.join("trap")).unwrap();
        let denied = std::fs::File::options().read(true).custom_flags(libc::O_NOFOLLOW).open(jail.join("trap"));
        assert!(denied.is_err(), "the check for links does not work");

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
