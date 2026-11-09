//! A browser VM's guest: no shell, no file access, only the broker's commands.
//!
//! A browser VM holds one identity's session, and the agent may never reach
//! it. So this role serves a short list of page commands and nothing else:
//! there is no way to run a program, read a file, read cookies, evaluate a
//! script or open developer tools.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// On the kernel's command line of a browser VM.
pub const ROLE: &str = "magnus.role=browser";
const NODE: &str = "/usr/bin/node";
const SIDECAR: &str = "/usr/local/lib/magnus/browser/sidecar.js";
const CHROMIUM: &str = "/usr/bin/chromium";
const SOCKET_DIR: &str = "/run/browser";
const SOCKET: &str = "/run/browser/s";
const PROFILE: &str = "/workspace/profile";
const HOME: &str = "/tmp/browser-home";
/// Not root: the page process has nothing else on this machine to reach.
const BROWSER_UID: u32 = 1000;
const SIDECAR_RESTART_WAIT: Duration = Duration::from_secs(1);
const SIDECAR_START_WAIT: Duration = Duration::from_secs(45);
const SIDECAR_POLL: Duration = Duration::from_millis(100);
const MAX_REPLY_BYTES: u64 = 64 * 1024 * 1024;
/// One context: the VM belongs to a single identity or throwaway.
const RUN: &str = "vm";

/// Everything the broker may ask. `ping` and `sync` are the host's, for
/// knowing the VM is up and for flushing the profile before a stop.
pub const COMMANDS: [&str; 10] = ["navigate", "snapshot", "click", "type", "select", "back", "screenshot", "fill_secret", "close", "render_html"];

pub fn is_browser_vm() -> bool {
    std::fs::read_to_string("/proc/cmdline").is_ok_and(|c| c.split_whitespace().any(|a| a == ROLE))
}

fn start_sidecar() -> std::io::Result<std::process::Child> {
    std::fs::create_dir_all(HOME)?;
    std::fs::create_dir_all(PROFILE)?;
    std::fs::create_dir_all(SOCKET_DIR)?;
    for dir in [HOME, PROFILE, SOCKET_DIR] {
        std::os::unix::fs::chown(dir, Some(BROWSER_UID), Some(BROWSER_UID))?;
    }
    std::os::unix::fs::chown("/workspace", Some(BROWSER_UID), Some(BROWSER_UID))?;
    let mut cmd = Command::new(NODE);
    cmd.arg(SIDECAR)
        .arg("--socket")
        .arg(SOCKET)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", HOME)
        .env("MAGNUS_CHROMIUM", CHROMIUM)
        .env("MAGNUS_VM_PROFILE", PROFILE)
        .env("MAGNUS_VM_PROXY", crate::PROXY_URL)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .uid(BROWSER_UID)
        .gid(BROWSER_UID);
    cmd.spawn()
}

fn keep_sidecar_running() {
    loop {
        let _ = std::fs::remove_file(SOCKET);
        match start_sidecar() {
            Ok(mut child) => {
                let _ = child.wait();
            }
            Err(e) => eprintln!("magnus-guest: the browser did not start: {e}"),
        }
        std::thread::sleep(SIDECAR_RESTART_WAIT);
    }
}

/// What the broker said, with the sidecar's run name pinned to this VM's.
fn checked(req: &Value) -> Result<Value, String> {
    let op = req["op"].as_str().unwrap_or_default();
    if !COMMANDS.contains(&op) {
        return Err(format!("a browser VM does not offer {op:?}"));
    }
    let mut req = req.clone();
    req["run"] = json!(RUN);
    Ok(req)
}

fn sidecar(req: &Value) -> Result<Value, String> {
    let mut conn = UnixStream::connect(SOCKET).map_err(|_| "the browser is still starting".to_string())?;
    conn.write_all(format!("{req}\n").as_bytes()).map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(conn.take(MAX_REPLY_BYTES)).read_line(&mut reply).map_err(|e| e.to_string())?;
    serde_json::from_str(&reply).map_err(|e| format!("the browser's reply: {e}"))
}

fn answer(req: &Value) -> Value {
    match req["op"].as_str() {
        Some("ping") => return json!({ "ok": true }),
        Some("sync") => {
            unsafe { libc::sync() };
            return json!({ "ok": true });
        }
        _ => {}
    }
    match checked(req).and_then(|r| sidecar(&r)) {
        Ok(v) => v,
        Err(error) => json!({ "error": error }),
    }
}

/// Up once the browser can take commands, so a first command never finds it
/// still starting. Serves anyway after this long, to say what went wrong.
fn wait_for_sidecar() {
    let started = Instant::now();
    while UnixStream::connect(SOCKET).is_err() && started.elapsed() < SIDECAR_START_WAIT {
        std::thread::sleep(SIDECAR_POLL);
    }
}

pub fn serve() {
    std::thread::spawn(keep_sidecar_running);
    wait_for_sidecar();
    let lfd = crate::listen(crate::PORT);
    loop {
        let fd = unsafe { libc::accept(lfd, std::ptr::null_mut(), std::ptr::null_mut()) };
        if fd < 0 {
            continue;
        }
        std::thread::spawn(move || {
            let stream = unsafe { std::fs::File::from_raw_fd(fd) };
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                return;
            }
            let reply = match serde_json::from_str::<Value>(&line) {
                Ok(req) => answer(&req),
                Err(e) => json!({ "error": format!("bad request: {e}") }),
            };
            let mut w = &stream;
            let _ = w.write_all(format!("{reply}\n").as_bytes());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_but_page_commands_is_offered() {
        for op in ["exec", "read", "write", "evaluate", "cookies", "devtools", "cdp", "storage_state", ""] {
            assert!(checked(&json!({ "op": op })).is_err(), "{op} must be refused");
        }
        assert!(checked(&json!({ "op": "exec", "command": "cat /workspace/profile/Default/Cookies" })).is_err());
    }

    #[test]
    fn a_request_cannot_name_another_context() {
        let req = checked(&json!({ "op": "navigate", "url": "https://a.example/", "run": "other" })).unwrap();
        assert_eq!(req["run"], RUN);
    }
}
