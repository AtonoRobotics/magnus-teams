//! The guest side of the sandbox. As PID 1 it mounts the filesystems, brings
//! up loopback (the only interface; the VM has no network device), and runs the
//! agent: a vsock server on port 52 that runs commands and moves files for the
//! kernel, one JSON request per connection. It holds no credentials.
//!
//! Booted for a contract check (`magnus.check=1` on the kernel command line),
//! it mounts the workspace without exec, gives commands a fresh home instead
//! of /workspace, and has Python ignore user site-packages and put paths from
//! outside the image after the image's own (`sitecustomize.py` in the image),
//! so the checking tool is the image's, never one the work put there.

use base64::Engine;
use serde_json::{json, Value};
use std::ffi::CString;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PORT: u32 = 52;
const MAX_OUT: usize = 200_000;

fn mount(src: &str, target: &str, fstype: &str, flags: libc::c_ulong, data: &str) {
    let _ = std::fs::create_dir_all(target);
    let (s, t, f, d) = (CString::new(src).unwrap(), CString::new(target).unwrap(), CString::new(fstype).unwrap(), CString::new(data).unwrap());
    let rc = unsafe { libc::mount(s.as_ptr(), t.as_ptr(), f.as_ptr(), flags, d.as_ptr() as *const libc::c_void) };
    if rc != 0 {
        eprintln!("magnus-guest: mount {target}: {}", std::io::Error::last_os_error());
    }
}

fn loopback_up() {
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0);
        if fd < 0 {
            return;
        }
        let mut req: libc::ifreq = std::mem::zeroed();
        for (i, b) in b"lo\0".iter().enumerate() {
            req.ifr_name[i] = *b as libc::c_char;
        }
        if libc::ioctl(fd, libc::SIOCGIFFLAGS as _, &mut req) == 0 {
            req.ifr_ifru.ifru_flags |= libc::IFF_UP as libc::c_short;
            libc::ioctl(fd, libc::SIOCSIFFLAGS as _, &req);
        }
        libc::close(fd);
    }
}

fn check_vm() -> bool {
    std::fs::read_to_string("/proc/cmdline").is_ok_and(|c| c.split_whitespace().any(|a| a == "magnus.check=1"))
}

fn init() {
    mount("proc", "/proc", "proc", 0, "");
    mount("sysfs", "/sys", "sysfs", 0, "");
    mount("devtmpfs", "/dev", "devtmpfs", 0, "");
    mount("devpts", "/dev/pts", "devpts", 0, "");
    mount("tmpfs", "/tmp", "tmpfs", 0, "mode=1777");
    mount("tmpfs", "/run", "tmpfs", 0, "mode=755");
    if check_vm() {
        mount("/dev/vdb", "/workspace", "ext4", libc::MS_NOEXEC | libc::MS_NOSUID | libc::MS_NODEV, "");
    } else {
        mount("/dev/vdb", "/workspace", "ext4", 0, "");
    }
    loopback_up();
    let _ = std::fs::write("/proc/sys/kernel/hostname", "sandbox");
}

fn listen() -> RawFd {
    unsafe {
        let fd = libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM, 0);
        assert!(fd >= 0, "vsock socket: {}", std::io::Error::last_os_error());
        let mut addr: libc::sockaddr_vm = std::mem::zeroed();
        addr.svm_family = libc::AF_VSOCK as libc::sa_family_t;
        addr.svm_port = PORT;
        addr.svm_cid = libc::VMADDR_CID_ANY;
        let rc = libc::bind(fd, &addr as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_vm>() as u32);
        assert!(rc == 0, "vsock bind: {}", std::io::Error::last_os_error());
        assert!(libc::listen(fd, 64) == 0, "vsock listen");
        fd
    }
}

fn truncate(mut b: Vec<u8>) -> (String, bool) {
    let cut = b.len() > MAX_OUT;
    b.truncate(MAX_OUT);
    (String::from_utf8_lossy(&b).into_owned(), cut)
}

fn exec(req: &Value) -> Value {
    let command = req["command"].as_str().unwrap_or_default();
    let timeout = Duration::from_secs(req["timeout_s"].as_u64().unwrap_or(120).clamp(1, 3600));
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(command)
        .current_dir("/workspace")
        .env_clear()
        .env("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
        .env("HOME", "/workspace")
        .env("LANG", "C.UTF-8")
        .env("DISPLAY", ":0");
    if check_vm() {
        let _ = std::fs::create_dir_all("/tmp/home");
        cmd.env("HOME", "/tmp/home").env("PYTHONNOUSERSITE", "1").env("PYTHONSAFEPATH", "1").env("MAGNUS_CHECK", "1");
    }
    let mut child = match cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0).spawn() {
        Ok(c) => c,
        Err(e) => return json!({ "error": format!("could not start the shell: {e}") }),
    };
    // Read output as it comes; background processes may keep the pipes open
    // after the shell exits, so stop collecting shortly after it does.
    let collect = |mut r: Box<dyn Read + Send>| {
        let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let b = buf.clone();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            while let Ok(n) = r.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let mut g = b.lock().unwrap();
                if g.len() < MAX_OUT + 1 {
                    g.extend_from_slice(&chunk[..n]);
                }
            }
        });
        buf
    };
    let so = collect(Box::new(child.stdout.take().unwrap()));
    let se = collect(Box::new(child.stderr.take().unwrap()));
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() > timeout => {
                timed_out = true;
                unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
                break child.wait().ok();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break None,
        }
    };
    std::thread::sleep(Duration::from_millis(150));
    let (stdout, c1) = truncate(so.lock().unwrap().clone());
    let (stderr, c2) = truncate(se.lock().unwrap().clone());
    json!({
        "exit_code": status.and_then(|s| s.code()),
        "stdout": stdout, "stderr": stderr,
        "timed_out": timed_out, "truncated": c1 || c2
    })
}

fn resolve(path: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::path::Path::new("/workspace").join(p)
    }
}

fn handle(req: Value) -> Value {
    let b64 = base64::engine::general_purpose::STANDARD;
    match req["op"].as_str().unwrap_or_default() {
        "ping" => json!({ "ok": true }),
        "exec" => exec(&req),
        "write" => {
            let p = resolve(req["path"].as_str().unwrap_or_default());
            let data = match b64.decode(req["data_b64"].as_str().unwrap_or_default()) {
                Ok(d) => d,
                Err(e) => return json!({ "error": format!("bad base64: {e}") }),
            };
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            match std::fs::write(&p, data) {
                Ok(()) => json!({ "ok": true, "path": p }),
                Err(e) => json!({ "error": format!("{}: {e}", p.display()) }),
            }
        }
        "read" => {
            let p = resolve(req["path"].as_str().unwrap_or_default());
            match std::fs::read(&p) {
                Ok(d) => json!({ "data_b64": b64.encode(d), "path": p }),
                Err(e) => json!({ "error": format!("{}: {e}", p.display()) }),
            }
        }
        "sync" => {
            unsafe { libc::sync() };
            json!({ "ok": true })
        }
        other => json!({ "error": format!("unknown op {other}") }),
    }
}

fn serve() {
    let lfd = listen();
    loop {
        let fd = unsafe { libc::accept(lfd, std::ptr::null_mut(), std::ptr::null_mut()) };
        if fd < 0 {
            continue;
        }
        std::thread::spawn(move || {
            let stream = unsafe { std::fs::File::from_raw_fd(fd) };
            let mut line = String::new();
            let mut r = BufReader::new(&stream);
            if r.read_line(&mut line).is_err() {
                return;
            }
            let resp = match serde_json::from_str::<Value>(&line) {
                Ok(req) => handle(req),
                Err(e) => json!({ "error": format!("bad request: {e}") }),
            };
            let mut w = &stream;
            let _ = w.write_all(format!("{resp}\n").as_bytes());
        });
    }
}

fn main() {
    if std::process::id() != 1 {
        serve();
        return;
    }
    init();
    loop {
        match unsafe { libc::fork() } {
            0 => {
                serve();
                std::process::exit(0);
            }
            pid if pid > 0 => {
                // Reap everything, including orphans reparented to PID 1; start
                // the agent again if it dies.
                loop {
                    let mut st = 0;
                    let got = unsafe { libc::waitpid(-1, &mut st, 0) };
                    if got == pid {
                        break;
                    }
                }
            }
            _ => std::thread::sleep(Duration::from_secs(1)),
        }
    }
}
