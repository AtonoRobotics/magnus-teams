//! Carries one connection between Magnus and a computer's guest agent. The
//! socket lives in a directory the VM's own user controls, so root never
//! opens it: this runs as a fresh process that first locks itself into that
//! chroot as that user, where a planted link can only point inside the jail.

use anyhow::{bail, Context, Result};
use std::ffi::CString;
use std::io::{BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

pub const GUEST_PORT: u32 = 52;
/// The guest serves its desktop here, and only over vsock.
pub const DISPLAY_PORT: u32 = 5900;
/// Set by `computers`, inside the chroot.
pub const VSOCK_IN_JAIL: &str = "/v.sock";
const STDIN: i32 = 0;
const STDOUT: i32 = 1;

fn check(rc: i32, what: &str) -> Result<()> {
    if rc != 0 {
        bail!("{what}: {}", std::io::Error::last_os_error());
    }
    Ok(())
}

/// Irreversibly becomes `uid` inside `root`.
fn confine(root: &Path, uid: u32) -> Result<()> {
    let dir = CString::new(root.as_os_str().as_encoded_bytes()).context("the jail's path")?;
    let slash = CString::new("/").context("the root path")?;
    unsafe {
        check(libc::chroot(dir.as_ptr()), "chroot")?;
        check(libc::chdir(slash.as_ptr()), "chdir")?;
        check(libc::setgroups(0, std::ptr::null()), "setgroups")?;
        check(libc::setresgid(uid, uid, uid), "setgid")?;
        check(libc::setresuid(uid, uid, uid), "setuid")?;
    }
    Ok(())
}

/// Firecracker reads the `CONNECT` line once, without waiting, as soon as the
/// stream has any bytes, and drops the stream if the line is not all there:
/// the relay then sees "Connection reset by peer" or "Broken pipe". So the
/// line goes in one write, never in pieces as `writeln!` sends it.
fn send_connect(to: &mut impl Write, port: u32) -> std::io::Result<()> {
    to.write_all(format!("CONNECT {port}\n").as_bytes())
}

fn connect_guest(port: u32) -> Result<(UnixStream, BufReader<UnixStream>)> {
    let mut guest = UnixStream::connect(VSOCK_IN_JAIL).context("reach the computer")?;
    send_connect(&mut guest, port)?;
    let mut reader = BufReader::new(guest.try_clone()?);
    let mut ack = String::new();
    reader.read_line(&mut ack)?;
    if !ack.starts_with("OK") {
        bail!("the guest agent is not listening");
    }
    Ok((guest, reader))
}

/// Runs with the client's connection as stdin and stdout.
pub fn run(root: &Path, uid: u32, port: u32) -> Result<()> {
    confine(root, uid)?;
    // Duplicates of the one client socket, as two owned handles.
    let client_in = UnixStream::from(unsafe { OwnedFd::from_raw_fd(STDIN) });
    let mut client_out = UnixStream::from(unsafe { OwnedFd::from_raw_fd(STDOUT) });
    let (guest, mut from_guest) = match connect_guest(port) {
        Ok(pair) => pair,
        Err(e) => {
            writeln!(client_out, "{}", serde_json::json!({ "ok": false, "error": format!("{e:#}") }))?;
            return Ok(());
        }
    };
    writeln!(client_out, "{}", serde_json::json!({ "ok": true }))?;
    let mut to_guest = guest.try_clone()?;
    let upstream = std::thread::spawn(move || {
        let mut client_in = client_in;
        let _ = std::io::copy(&mut client_in, &mut to_guest);
        let _ = to_guest.shutdown(Shutdown::Write);
    });
    let _ = std::io::copy(&mut from_guest, &mut client_out);
    let _ = client_out.shutdown(Shutdown::Both);
    let _ = guest.shutdown(Shutdown::Both);
    let _ = upstream.join();
    Ok(())
}

const API_SOCKET: &str = "/run/firecracker.socket";
const API_WAIT: Duration = Duration::from_secs(5);
const API_POLL: Duration = Duration::from_millis(20);
pub const SNAPSHOT_STATE: &str = "snapshot.state";
pub const SNAPSHOT_MEMORY: &str = "snapshot.mem";

fn api_call(method: &str, path: &str, body: &str) -> Result<()> {
    let started = Instant::now();
    let mut s = loop {
        match UnixStream::connect(API_SOCKET) {
            Ok(s) => break s,
            Err(e) if started.elapsed() > API_WAIT => return Err(e).context("reach firecracker's API"),
            Err(_) => std::thread::sleep(API_POLL),
        }
    };
    write!(s, "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len())?;
    let mut reader = BufReader::new(s);
    let mut status = String::new();
    reader.read_line(&mut status)?;
    if !status.split_whitespace().nth(1).is_some_and(|c| c.starts_with('2')) {
        bail!("firecracker refused {method} {path}: {}", status.trim());
    }
    Ok(())
}

/// Drives firecracker's API as the VM's user, inside its chroot.
pub fn api(root: &Path, uid: u32, what: &str) -> Result<()> {
    confine(root, uid)?;
    match what {
        "snapshot" => {
            api_call("PATCH", "/vm", r#"{"state":"Paused"}"#)?;
            api_call("PUT", "/snapshot/create", &format!(r#"{{"snapshot_type":"Full","snapshot_path":"{SNAPSHOT_STATE}","mem_file_path":"{SNAPSHOT_MEMORY}"}}"#))
        }
        "restore" => api_call(
            "PUT",
            "/snapshot/load",
            &format!(r#"{{"snapshot_path":"{SNAPSHOT_STATE}","mem_backend":{{"backend_path":"{SNAPSHOT_MEMORY}","backend_type":"File"}},"resume_vm":true}}"#),
        ),
        _ => bail!("no such API step"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each `write` call it is given, as Firecracker's socket would receive it.
    #[derive(Default)]
    struct Writes(Vec<Vec<u8>>);

    impl Write for Writes {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.push(buf.to_vec());
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn the_connect_line_reaches_firecracker_in_one_write() {
        let mut to = Writes::default();
        send_connect(&mut to, GUEST_PORT).unwrap();
        assert_eq!(to.0, vec![b"CONNECT 52\n".to_vec()], "the line went in pieces, and Firecracker drops a stream whose line is cut");
    }
}
