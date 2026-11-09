//! Chromium reads hostile pages on the machine that holds the vault, so its
//! own sandbox must be on and it must not share the host's files or network.
//! The checks look at the real processes from outside, while a page is open.
//! Needs Chromium and `config/install-browser-jail.sh`.

use magnus_scratch::chromium;
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const SOCKET_WAIT: Duration = Duration::from_secs(10);
const REPLY_WAIT: Duration = Duration::from_secs(90);
const BWRAP: &str = "/usr/local/lib/magnus/bwrap";
const PAGE: &str = "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\nconnection: close\r\n\r\n<p>jailed page text</p>";

struct KillOnDrop(Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn serve_page() -> io::Result<u16> {
    let server = TcpListener::bind("127.0.0.1:0")?;
    let port = server.local_addr()?.port();
    std::thread::spawn(move || {
        for conn in server.incoming().flatten() {
            let mut conn = conn;
            let mut buf = [0u8; 4096];
            let _ = conn.read(&mut buf);
            let _ = conn.write_all(PAGE.as_bytes());
        }
    });
    Ok(port)
}

fn ask(socket: &Path, request: &str) -> io::Result<serde_json::Value> {
    let mut conn = UnixStream::connect(socket)?;
    conn.set_read_timeout(Some(REPLY_WAIT))?;
    writeln!(conn, "{request}")?;
    let mut line = String::new();
    BufReader::new(conn).read_line(&mut line)?;
    Ok(serde_json::from_str(&line)?)
}

fn proc_file(pid: u32, name: &str) -> String {
    std::fs::read(format!("/proc/{pid}/{name}")).map(|b| String::from_utf8_lossy(&b).replace('\0', " ")).unwrap_or_default()
}

fn namespace(pid: u32, kind: &str) -> io::Result<String> {
    Ok(std::fs::read_link(format!("/proc/{pid}/ns/{kind}"))?.display().to_string())
}

/// Every process under `root`, with its command line.
fn descendants(root: u32) -> io::Result<Vec<(u32, String)>> {
    let mut parents = HashMap::new();
    for e in std::fs::read_dir("/proc")?.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let stat = proc_file(pid, "stat");
        let Some(rest) = stat.rsplit(") ").next() else { continue };
        if let Some(ppid) = rest.split(' ').nth(1).and_then(|s| s.parse::<u32>().ok()) {
            parents.insert(pid, ppid);
        }
    }
    let under = |mut p: u32| {
        while let Some(&up) = parents.get(&p) {
            if up == root {
                return true;
            }
            p = up;
        }
        false
    };
    Ok(parents.keys().filter(|&&p| under(p)).map(|&p| (p, proc_file(p, "cmdline"))).collect())
}

#[test]
fn chromium_runs_sandboxed_with_no_host_files_or_network() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault.key");
    std::fs::write(&vault, "canary").unwrap();
    let socket = dir.path().join("b.sock");
    let port = serve_page().unwrap();
    let child = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../browser/sidecar.js"))
        .arg("--socket")
        .arg(&socket)
        .args(["--allow", "127.0.0.1"])
        .env("MAGNUS_CHROMIUM", chromium())
        .env("MAGNUS_BWRAP", BWRAP)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let sidecar = KillOnDrop(child);
    let started = Instant::now();
    while UnixStream::connect(&socket).is_err() {
        assert!(started.elapsed() < SOCKET_WAIT, "the sidecar did not start");
        std::thread::sleep(Duration::from_millis(50));
    }

    // The page arrives, so the only route out (relay, then proxy) works.
    let reply = ask(&socket, &format!(r#"{{"op":"navigate","run":"r","url":"http://127.0.0.1:{port}/"}}"#)).unwrap();
    assert!(reply["text"].as_str().unwrap_or_default().contains("jailed page text"), "the page did not load: {reply}");

    let procs = descendants(sidecar.0.id()).unwrap();
    // A renderer's command line can carry the debugging flag too; only the main process has no --type.
    let main = procs.iter().find(|(_, c)| c.contains("--remote-debugging-pipe") && !c.contains("relay.js") && !c.contains("--type=")).expect("no Chromium process");
    let renderers: Vec<_> = procs.iter().filter(|(_, c)| c.contains("--type=renderer")).collect();
    assert!(!renderers.is_empty(), "no renderer process");
    for (_, command) in procs.iter().filter(|(_, c)| c.contains("chrome")) {
        assert!(!command.contains("--no-sandbox"), "a Chromium process has its sandbox off: {command}");
    }
    let host = std::process::id();
    assert_ne!(namespace(main.0, "net").unwrap(), namespace(host, "net").unwrap(), "Chromium shares the host's network");
    assert_ne!(namespace(main.0, "mnt").unwrap(), namespace(host, "mnt").unwrap(), "Chromium shares the host's files");
    for (renderer, _) in &renderers {
        assert_ne!(namespace(*renderer, "pid").unwrap(), namespace(main.0, "pid").unwrap(), "a renderer is not in Chromium's own sandbox");
        assert!(proc_file(*renderer, "status").contains("Seccomp:\t2"), "a renderer has no seccomp filter");
    }
    let seen_from_jail = format!("/proc/{}/root{}", main.0, vault.display());
    assert!(!Path::new(&seen_from_jail).exists(), "Chromium can see a file beside the vault");
    // Chromium's own install can sit under the home folder, so /home can exist
    // in the jail. Nothing else there may.
    let home_canary = tempfile::tempdir_in(std::env::var("HOME").unwrap()).unwrap();
    let in_home = home_canary.path().join("vault.key");
    std::fs::write(&in_home, "canary").unwrap();
    let repo_file = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml").canonicalize().unwrap();
    for host_file in [in_home.as_path(), repo_file.as_path()] {
        assert!(host_file.exists());
        let seen = format!("/proc/{}/root{}", main.0, host_file.display());
        assert!(!Path::new(&seen).exists(), "Chromium can see {}", host_file.display());
    }
}
