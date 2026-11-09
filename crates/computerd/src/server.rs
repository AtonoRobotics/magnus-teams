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
    RestoreBackup {
        id: String,
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
    Status {
        id: String,
    },
    List,
    /// Turns this connection into a stream to the guest agent.
    Connect {
        id: String,
    },
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
        Request::RestoreBackup { id: raw } => c.restore_backup(&id(&raw)?)?,
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
        Request::Stop { id: raw } => {
            c.stop(&id(&raw)?)?;
            json!({})
        }
        Request::Destroy { id: raw, delete_volume } => {
            c.destroy(&id(&raw)?, delete_volume)?;
            json!({})
        }
        Request::Status { id: raw } => json!({ "state": c.status(&id(&raw)?)? }),
        Request::List => json!({ "computers": c.list()?.iter().map(|i| i.as_str().to_string()).collect::<Vec<_>>() }),
        Request::Connect { .. } => anyhow::bail!("connect is handled by the connection itself"),
    })
}

fn relay(c: &Computers, conn: UnixStream, raw: &str) -> Result<()> {
    let (root, uid) = c.relay_target(&ComputerId::parse(raw)?)?;
    conn.set_read_timeout(None)?;
    let out = conn.try_clone()?;
    // Its own process, because it gives up root and this one cannot.
    let status = Command::new(std::env::current_exe().context("find this program")?)
        .env_clear()
        .arg("relay")
        .arg(root)
        .arg(uid.to_string())
        .stdin(Stdio::from(OwnedFd::from(conn)))
        .stdout(Stdio::from(OwnedFd::from(out)))
        .stderr(Stdio::null())
        .status()
        .context("run the relay")?;
    ensure!(status.success(), "the relay could not enter the computer's jail");
    Ok(())
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
                Ok(Request::Connect { id }) => {
                    let reply_on = conn.try_clone();
                    if let (Err(e), Ok(mut c)) = (relay(&computers, conn, &id), reply_on) {
                        let _ = writeln!(c, "{}", json!({ "ok": false, "error": format!("{e:#}") }));
                    }
                }
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
    }
}
