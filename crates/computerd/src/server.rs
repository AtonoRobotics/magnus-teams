//! The socket Magnus talks to: one JSON request, one JSON reply, per connection.

use crate::computers::Computers;
use crate::config::Config;
use crate::id::ComputerId;
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{chown, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const MAX_REQUEST_BYTES: u64 = 4096;
const MAX_CONNECTIONS: usize = 64;
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
}

pub fn dispatch(c: &Computers, req: Request) -> Result<Value> {
    let id = |raw: &str| ComputerId::parse(raw);
    Ok(match req {
        Request::Create { id: raw } => json!({ "uid": c.create(&id(&raw)?)? }),
        Request::Start { id: raw } => {
            c.start(&id(&raw)?)?;
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
    })
}

fn answer(c: &Computers, conn: &UnixStream) -> Value {
    let mut line = String::new();
    let read = conn.set_read_timeout(Some(READ_TIMEOUT)).and_then(|_| BufReader::new((&*conn).take(MAX_REQUEST_BYTES)).read_line(&mut line));
    let result = read.map_err(anyhow::Error::from).and_then(|_| serde_json::from_str::<Request>(&line).context("bad request")).and_then(|req| dispatch(c, req));
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
            let reply = answer(&computers, &conn);
            let _ = writeln!(conn, "{reply}");
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
