//! One computer's way out: listens on the guest's gateway, inside the
//! computer's network namespace, and carries each connection to Magnus's
//! egress proxy. It runs as the computer's own uid, so the proxy can tell
//! whose connection it is from the socket and need trust no bytes that arrive.

use anyhow::{bail, Context, Result};
use std::io::ErrorKind;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const GATEWAY: &str = "10.200.0.1";
pub const GUEST_NETMASK_BITS: u8 = 30;
pub const PROXY_PORT: u16 = 3128;
const MAX_CONNECTIONS: usize = 512;
const VM_CHECK: Duration = Duration::from_secs(1);

fn check(rc: i32, what: &str) -> Result<()> {
    if rc != 0 {
        bail!("{what}: {}", std::io::Error::last_os_error());
    }
    Ok(())
}

fn carry(from_guest: TcpStream, to_proxy: UnixStream) {
    let (Ok(mut a), Ok(mut b)) = (from_guest.try_clone(), to_proxy.try_clone()) else { return };
    let up = std::thread::spawn(move || {
        let _ = std::io::copy(&mut a, &mut b);
        let _ = b.shutdown(Shutdown::Write);
    });
    let (mut guest, mut proxy) = (from_guest, to_proxy);
    let _ = std::io::copy(&mut proxy, &mut guest);
    let _ = guest.shutdown(Shutdown::Write);
    let _ = up.join();
}

fn all_gone(pids: &[i32]) -> bool {
    pids.iter().all(|p| {
        let alive = unsafe { libc::kill(*p, 0) } == 0;
        !alive && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    })
}

pub fn run(netns: &Path, uid: u32, socket_gid: u32, proxy: &Path, pids: &[i32]) -> Result<()> {
    let ns = std::fs::File::open(netns).context("open the network namespace")?;
    check(unsafe { libc::setns(ns.as_raw_fd(), libc::CLONE_NEWNET) }, "enter the network namespace")?;
    let listener = TcpListener::bind((GATEWAY, PROXY_PORT)).context("listen on the gateway")?;
    let groups = [socket_gid];
    unsafe {
        check(libc::setgroups(groups.len(), groups.as_ptr()), "setgroups")?;
        check(libc::setresgid(uid, uid, uid), "setgid")?;
        check(libc::setresuid(uid, uid, uid), "setuid")?;
    }
    let watched = pids.to_vec();
    std::thread::spawn(move || loop {
        std::thread::sleep(VM_CHECK);
        if all_gone(&watched) {
            std::process::exit(0);
        }
    });
    let open = Arc::new(AtomicUsize::new(0));
    for conn in listener.incoming() {
        let conn = match conn {
            Ok(c) => c,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        if open.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            open.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let (open, proxy) = (open.clone(), proxy.to_path_buf());
        std::thread::spawn(move || {
            // With Magnus away there is no way out at all.
            if let Ok(to_proxy) = UnixStream::connect(&proxy) {
                let _ = conn.set_nodelay(true);
                carry(conn, to_proxy);
            }
            open.fetch_sub(1, Ordering::SeqCst);
        });
    }
    Ok(())
}
