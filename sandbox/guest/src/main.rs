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
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PORT: u32 = 52;
const MAX_OUT: usize = 200_000;
const MIN_ENTROPY_BYTES: usize = 32;
/// `_IOWR('X', 119, int)` and `_IOWR('X', 120, int)` from linux/fs.h.
const FIFREEZE: u64 = 0xC004_5877;
const FITHAW: u64 = 0xC004_5878;
/// A host that froze the volume and went away cannot leave it frozen.
const MAX_FREEZE: Duration = Duration::from_secs(120);
const OVERLAY_DRIVE: &str = "/dev/vdc";
const ERASE_BUFFER_BYTES: usize = 1 << 20;
/// `_IOW('R', 0x03, int[2])` and `_IO('R', 0x07)` from linux/random.h.
const RNDADDENTROPY: u64 = 0x4008_5203;
const RNDRESEEDCRNG: u64 = 0x5207;

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

fn freeze(frozen: bool) -> Value {
    use std::os::fd::AsRawFd;
    let Ok(volume) = std::fs::File::open("/workspace") else { return json!({ "error": "no volume" }) };
    let request = if frozen { FIFREEZE } else { FITHAW };
    if unsafe { libc::ioctl(volume.as_raw_fd(), request as _, 0) } != 0 {
        return json!({ "error": format!("{}", std::io::Error::last_os_error()) });
    }
    if frozen {
        std::thread::spawn(|| {
            std::thread::sleep(MAX_FREEZE);
            let _ = freeze(false);
        });
    }
    json!({ "ok": true })
}

/// Zeroes a file before unlinking it, since a freed block would otherwise
/// stay readable in the drive image and in every later backup of it.
fn erase(path: &std::path::Path) -> Value {
    let run = || -> std::io::Result<()> {
        let mut file = std::fs::File::options().write(true).custom_flags(libc::O_NOFOLLOW).open(path)?;
        let mut left = file.metadata()?.len();
        let zeros = vec![0u8; ERASE_BUFFER_BYTES];
        while left > 0 {
            let n = left.min(ERASE_BUFFER_BYTES as u64) as usize;
            file.write_all(&zeros[..n])?;
            left -= n as u64;
        }
        file.sync_all()?;
        std::fs::remove_file(path)?;
        unsafe { libc::sync() };
        Ok(())
    };
    match run() {
        Ok(()) => json!({ "ok": true, "path": path }),
        Err(e) => json!({ "error": format!("{}: {e}", path.display()) }),
    }
}

/// A restored snapshot shares its clock, machine id, entropy and name with
/// every other restore of it until this runs. The values come from the host.
fn reset(req: &Value) -> Value {
    let seconds = req["unix_seconds"].as_i64().unwrap_or_default();
    let machine_id = req["machine_id"].as_str().unwrap_or_default();
    let hostname = req["hostname"].as_str().unwrap_or_default();
    let Ok(entropy) = base64::engine::general_purpose::STANDARD.decode(req["entropy_b64"].as_str().unwrap_or_default()) else {
        return json!({ "error": "the entropy is not base64" });
    };
    if seconds <= 0 || machine_id.is_empty() || hostname.is_empty() || entropy.len() < MIN_ENTROPY_BYTES {
        return json!({ "error": "a reset needs the time, a machine id, a name and entropy" });
    }
    let clock = libc::timeval { tv_sec: seconds, tv_usec: 0 };
    if unsafe { libc::settimeofday(&clock, std::ptr::null()) } != 0 {
        return json!({ "error": format!("set the clock: {}", std::io::Error::last_os_error()) });
    }
    // The kernel mixes this into the generator and reseeds from it at once.
    let mut pool = Vec::with_capacity(8 + entropy.len());
    pool.extend_from_slice(&((entropy.len() * 8) as i32).to_ne_bytes());
    pool.extend_from_slice(&(entropy.len() as i32).to_ne_bytes());
    pool.extend_from_slice(&entropy);
    let reseeded = std::fs::File::options().write(true).open("/dev/urandom").map(|f| {
        use std::os::fd::AsRawFd;
        unsafe { libc::ioctl(f.as_raw_fd(), RNDADDENTROPY as _, pool.as_ptr()) == 0 && libc::ioctl(f.as_raw_fd(), RNDRESEEDCRNG as _) == 0 }
    });
    if !matches!(reseeded, Ok(true)) {
        return json!({ "error": "the guest's randomness was not reset" });
    }
    if std::fs::write("/etc/machine-id", format!("{machine_id}\n")).is_err() || std::fs::write("/proc/sys/kernel/hostname", hostname).is_err() {
        return json!({ "error": "the guest's identity was not reset" });
    }
    json!({ "ok": true })
}

fn check_vm() -> bool {
    std::fs::read_to_string("/proc/cmdline").is_ok_and(|c| c.split_whitespace().any(|a| a == "magnus.check=1"))
}

fn checked_mount(source: &str, target: &str, fstype: &str, flags: libc::c_ulong, data: &str) -> Result<(), String> {
    let c = |s: &str| CString::new(s).map_err(|e| e.to_string());
    let (src, tgt, fs, d) = (c(source)?, c(target)?, c(fstype)?, c(data)?);
    if unsafe { libc::mount(src.as_ptr(), tgt.as_ptr(), fs.as_ptr(), flags, d.as_ptr().cast()) } != 0 {
        return Err(format!("mount {target}: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

/// A computer's root: the read-only image with its own writable layer on top.
/// /dev comes along, and everything else is mounted over the new root.
fn overlay_root() -> Result<(), String> {
    checked_mount("tmpfs", "/mnt", "tmpfs", 0, "mode=700")?;
    for d in ["/mnt/drive", "/mnt/root"] {
        std::fs::create_dir(d).map_err(|e| format!("{d}: {e}"))?;
    }
    checked_mount(OVERLAY_DRIVE, "/mnt/drive", "ext4", 0, "")?;
    for d in ["/mnt/drive/upper", "/mnt/drive/work"] {
        std::fs::create_dir_all(d).map_err(|e| format!("{d}: {e}"))?;
    }
    checked_mount("overlay", "/mnt/root", "overlay", 0, "lowerdir=/,upperdir=/mnt/drive/upper,workdir=/mnt/drive/work")?;
    checked_mount("/dev", "/mnt/root/dev", "", libc::MS_MOVE, "")?;
    std::env::set_current_dir("/mnt/root").map_err(|e| format!("enter the new root: {e}"))?;
    let here = CString::new(".").map_err(|e| e.to_string())?;
    // The old root ends up stacked under the new one, then is let go.
    if unsafe { libc::syscall(libc::SYS_pivot_root, here.as_ptr(), here.as_ptr()) } != 0 {
        return Err(format!("pivot_root: {}", std::io::Error::last_os_error()));
    }
    if unsafe { libc::umount2(here.as_ptr(), libc::MNT_DETACH) } != 0 {
        return Err(format!("let go of the old root: {}", std::io::Error::last_os_error()));
    }
    std::env::set_current_dir("/").map_err(|e| format!("enter /: {e}"))
}

fn init() {
    mount("devtmpfs", "/dev", "devtmpfs", 0, "");
    if std::path::Path::new(OVERLAY_DRIVE).exists() {
        // Not worth running without: the apps and their replay assume it.
        if let Err(e) = overlay_root() {
            eprintln!("magnus-guest: the overlay did not mount: {e}");
            std::process::exit(1);
        }
    }
    mount("proc", "/proc", "proc", 0, "");
    mount("sysfs", "/sys", "sysfs", 0, "");
    mount("devpts", "/dev/pts", "devpts", 0, "");
    mount("tmpfs", "/tmp", "tmpfs", 0, "mode=1777");
    mount("tmpfs", "/run", "tmpfs", 0, "mode=755");
    if check_vm() {
        mount("/dev/vdb", "/workspace", "ext4", libc::MS_NOEXEC | libc::MS_NOSUID | libc::MS_NODEV, "");
    } else if checked_mount("/dev/vdb", "/workspace", "ext4", 0, "").is_err() {
        // A helper reading a hibernated computer's volume has it read-only,
        // and `noload` leaves its journal for the computer to replay.
        mount("/dev/vdb", "/workspace", "ext4", libc::MS_RDONLY, "noload");
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

/// The whole file, or the part of it asked for.
fn read_part(path: &std::path::Path, offset: Option<u64>, length: Option<u64>) -> std::io::Result<Vec<u8>> {
    let Some(length) = length else { return std::fs::read(path) };
    use std::io::Seek;
    let mut file = std::fs::File::open(path)?;
    file.seek(std::io::SeekFrom::Start(offset.unwrap_or(0)))?;
    let mut out = Vec::new();
    file.take(length).read_to_end(&mut out)?;
    Ok(out)
}

fn handle(req: Value) -> Value {
    let b64 = base64::engine::general_purpose::STANDARD;
    match req["op"].as_str().unwrap_or_default() {
        "ping" => json!({ "ok": true }),
        "reset" => reset(&req),
        "freeze" => freeze(true),
        "thaw" => freeze(false),
        "erase" => erase(&resolve(req["path"].as_str().unwrap_or_default())),
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
            let written =
                if req["append"] == true { std::fs::File::options().append(true).create(true).open(&p).and_then(|mut f| f.write_all(&data)) } else { std::fs::write(&p, data) };
            match written {
                Ok(()) => json!({ "ok": true, "path": p }),
                Err(e) => json!({ "error": format!("{}: {e}", p.display()) }),
            }
        }
        "read" => {
            let p = resolve(req["path"].as_str().unwrap_or_default());
            match read_part(&p, req["offset"].as_u64(), req["length"].as_u64()) {
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
