//! The socket Magnus talks to: one JSON request, one JSON reply, per connection.

use crate::computers::{Computers, Purpose};
use crate::config::Config;
use crate::id::ComputerId;
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::fs::{chown, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const MAX_REQUEST_BYTES: u64 = 4096;
const MAX_CONNECTIONS: usize = 64;
const BACKUP_EVERY: Duration = Duration::from_secs(3600);
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const SOCKET_MODE: u32 = 0o660;
const EGRESS_DIR_MODE: u32 = 0o2770;
/// So the socket is never open to others between bind and chmod.
const UMASK_BEFORE_BIND: libc::mode_t = 0o177;
const UMASK_AFTER_BIND: libc::mode_t = 0o077;

/// Unknown fields are refused: no request can carry a path or a size.
#[derive(Deserialize, Debug)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Create {
        id: String,
    },
    Start {
        id: String,
    },
    Hibernate {
        id: String,
    },
    Wake {
        id: String,
    },
    DiscardSnapshot {
        id: String,
    },
    Backup {
        id: String,
    },
    /// Where the egress proxy listens.
    EgressSocket,
    /// How many computers run and exist, against the machine's limits.
    Capacity,
    /// The computer that runs as this uid.
    Whose {
        uid: u32,
    },
    RestoreBackup {
        id: String,
    },
    /// A browser VM: the volume is its profile, and there is no shell.
    StartBrowser {
        id: String,
    },
    /// Whether the host can hold this volume with every block written.
    ZeroRoom {
        id: String,
    },
    /// Gives a stopped computer's zeroed blocks back to the host.
    DigHoles {
        id: String,
    },
    /// Encrypts a stopped browser's profile with a key the vault holds.
    SealVolume {
        id: String,
        key_b64: String,
    },
    UnsealVolume {
        id: String,
        key_b64: String,
    },
    /// A VM with only the volume, to reach a stopped computer's files.
    StartHelper {
        id: String,
        #[serde(default)]
        read_only: bool,
    },
    Pause {
        id: String,
    },
    Resume {
        id: String,
    },
    Stop {
        id: String,
    },
    Destroy {
        id: String,
        #[serde(default)]
        delete_volume: bool,
    },
    /// A reset that erases the files: the same computer with a new, empty
    /// volume. No path or size comes from the caller.
    EraseVolume {
        id: String,
    },
    Status {
        id: String,
    },
    List,
    /// Turns this connection into a stream to the guest agent.
    Connect {
        id: String,
    },
    /// The computer's desktop, as a raw stream to the guest.
    Display {
        id: String,
    },
    /// This build's release, base image and requests, for Magnus to compare
    /// with the computerd it shipped with. A computerd older than this
    /// request answers "bad request", which Magnus reads as older.
    Version,
}

fn key(b64: &str) -> Result<[u8; crate::seal::KEY_BYTES]> {
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD.decode(b64).context("a sealing key that is not base64")?;
    raw.try_into().map_err(|_| anyhow::anyhow!("a sealing key of the wrong length"))
}

pub fn dispatch(c: &Computers, req: Request) -> Result<Value> {
    let id = |raw: &str| ComputerId::parse(raw);
    Ok(match req {
        Request::Create { id: raw } => json!({ "uid": c.create(&id(&raw)?)? }),
        Request::Start { id: raw } => json!({ "overlay_fresh": c.start(&id(&raw)?, Purpose::Computer)? }),
        Request::Hibernate { id: raw } => {
            c.hibernate(&id(&raw)?)?;
            json!({})
        }
        Request::Wake { id: raw } => json!(c.wake(&id(&raw)?)?),
        Request::DiscardSnapshot { id: raw } => {
            c.discard_snapshot(&id(&raw)?)?;
            json!({})
        }
        Request::Backup { id: raw } => c.backup(&id(&raw)?)?,
        Request::EgressSocket => json!({ "socket": c.proxy_socket() }),
        Request::Capacity => c.capacity()?,
        Request::Whose { uid } => json!({ "id": c.whose_uid(uid)?.map(|i| i.as_str().to_string()) }),
        Request::RestoreBackup { id: raw } => c.restore_backup(&id(&raw)?)?,
        Request::StartBrowser { id: raw } => {
            c.start(&id(&raw)?, Purpose::Browser)?;
            json!({})
        }
        Request::SealVolume { id: raw, key_b64 } => {
            c.seal_volume(&id(&raw)?, &key(&key_b64)?)?;
            json!({})
        }
        Request::UnsealVolume { id: raw, key_b64 } => {
            c.unseal_volume(&id(&raw)?, &key(&key_b64)?)?;
            json!({})
        }
        Request::StartHelper { id: raw, read_only } => {
            c.start(&id(&raw)?, if read_only { Purpose::ReadHelper } else { Purpose::Helper })?;
            json!({})
        }
        Request::Pause { id: raw } => {
            c.pause(&id(&raw)?)?;
            json!({})
        }
        Request::Resume { id: raw } => {
            c.resume(&id(&raw)?)?;
            json!({})
        }
        Request::ZeroRoom { id: raw } => json!({ "room": c.room_to_zero(&id(&raw)?)? }),
        Request::DigHoles { id: raw } => json!({ "dug_bytes": c.dig_holes(&id(&raw)?)? }),
        Request::Stop { id: raw } => {
            c.stop(&id(&raw)?)?;
            json!({})
        }
        Request::Destroy { id: raw, delete_volume } => {
            c.destroy(&id(&raw)?, delete_volume)?;
            json!({})
        }
        Request::EraseVolume { id: raw } => {
            c.erase_volume(&id(&raw)?)?;
            json!({})
        }
        Request::Status { id: raw } => json!({ "state": c.status(&id(&raw)?)? }),
        Request::Version => crate::version::answer(std::env::current_exe().ok().as_deref().and_then(std::path::Path::parent), c.base_id().ok()),
        Request::List => json!({ "computers": c.list()?.iter().map(|i| i.as_str().to_string()).collect::<Vec<_>>() }),
        Request::Connect { .. } | Request::Display { .. } => anyhow::bail!("a connection is handled by the connection itself"),
    })
}

fn relay(c: &Computers, conn: UnixStream, raw: &str, port: u32) -> Result<()> {
    let (root, uid) = c.relay_target(&ComputerId::parse(raw)?)?;
    conn.set_read_timeout(None)?;
    let out = conn.try_clone()?;
    // Its own process, because it gives up root and this one cannot.
    let status = Command::new(std::env::current_exe().context("find this program")?)
        .env_clear()
        .arg("relay")
        .arg(root)
        .arg(uid.to_string())
        .arg(port.to_string())
        .stdin(Stdio::from(OwnedFd::from(conn)))
        .stdout(Stdio::from(OwnedFd::from(out)))
        .stderr(Stdio::null())
        .status()
        .context("run the relay")?;
    ensure!(status.success(), "the relay could not enter the computer's jail");
    Ok(())
}

fn relay_or_say(computers: &Computers, conn: UnixStream, id: &str, port: u32) {
    let reply_on = conn.try_clone();
    if let (Err(e), Ok(mut c)) = (relay(computers, conn, id, port), reply_on) {
        let _ = writeln!(c, "{}", json!({ "ok": false, "error": format!("{e:#}") }));
    }
}

/// A client sends one request and waits for the reply, so nothing past the
/// newline is lost to the buffer.
fn read_request(conn: &UnixStream) -> Result<Request> {
    let mut line = String::new();
    conn.set_read_timeout(Some(READ_TIMEOUT))?;
    BufReader::new(conn.take(MAX_REQUEST_BYTES)).read_line(&mut line)?;
    serde_json::from_str(&line).context("bad request")
}

fn answer(c: &Computers, req: Result<Request>) -> Value {
    let result = req.and_then(|req| dispatch(c, req));
    match result {
        Ok(mut v) => {
            v["ok"] = json!(true);
            v
        }
        Err(e) => json!({ "ok": false, "error": format!("{e:#}") }),
    }
}

/// Magnus's group makes the proxy's socket here, and forwarders reach it.
fn prepare_egress_dir(c: &Computers, gid: u32) -> Result<()> {
    let socket = c.proxy_socket();
    let dir = socket.parent().context("the egress directory")?;
    std::fs::create_dir_all(dir).context("make the egress directory")?;
    chown(dir, Some(0), Some(gid)).context("give the egress directory to the magnus group")?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(EGRESS_DIR_MODE)).context("set the egress directory's mode")
}

pub fn serve(cfg: Config) -> Result<()> {
    let socket = cfg.socket.clone();
    let gid = cfg.socket_gid;
    let computers = Arc::new(Computers::new(cfg));
    let _ = std::fs::remove_file(&socket);
    unsafe { libc::umask(UMASK_BEFORE_BIND) };
    let listener = UnixListener::bind(&socket).with_context(|| format!("listen on {}", socket.display()));
    // The jailer inherits this and builds the chroot under it.
    unsafe { libc::umask(UMASK_AFTER_BIND) };
    let listener = listener?;
    chown(&socket, Some(0), Some(gid)).context("give the socket to the magnus group")?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(SOCKET_MODE)).context("set the socket's mode")?;
    prepare_egress_dir(&computers, gid)?;
    // The VMs a previous run started are still running and are known from
    // their cgroups; what stopped ones left behind goes.
    if let Err(e) = computers.sweep() {
        eprintln!("magnus-computerd: {e:#}");
    }
    let keeper = computers.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(BACKUP_EVERY);
        if let Err(e) = keeper.backup_changed() {
            eprintln!("magnus-computerd: {e:#}");
        }
        if let Err(e) = keeper.purge(SystemTime::now()) {
            eprintln!("magnus-computerd: {e:#}");
        }
    });
    let open = Arc::new(AtomicUsize::new(0));
    for conn in listener.incoming() {
        let Ok(mut conn) = conn else { continue };
        if open.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            open.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let computers = computers.clone();
        let open = open.clone();
        std::thread::spawn(move || {
            match read_request(&conn) {
                Ok(Request::Connect { id }) => relay_or_say(&computers, conn, &id, crate::relay::GUEST_PORT),
                Ok(Request::Display { id }) => relay_or_say(&computers, conn, &id, crate::relay::DISPLAY_PORT),
                req => {
                    let reply = answer(&computers, req);
                    let _ = writeln!(conn, "{reply}");
                }
            }
            open.fetch_sub(1, Ordering::SeqCst);
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Request;

    #[test]
    fn a_request_cannot_carry_a_path_or_a_size() {
        for bad in [
            r#"{"op":"create","id":"a","path":"/etc/shadow"}"#,
            r#"{"op":"start","id":"a","memory_mib":99999}"#,
            r#"{"op":"start","id":"a","jailer":"/tmp/x"}"#,
            r#"{"op":"run","command":"sh"}"#,
        ] {
            assert!(serde_json::from_str::<Request>(bad).is_err(), "{bad} was accepted");
        }
        assert!(serde_json::from_str::<Request>(r#"{"op":"destroy","id":"a","delete_volume":true}"#).is_ok());
        assert!(serde_json::from_str::<Request>(r#"{"op":"capacity"}"#).is_ok(), "the machine cannot be asked its capacity");
        assert!(serde_json::from_str::<Request>(r#"{"op":"version"}"#).is_ok(), "the service cannot be asked its version");
    }
}
