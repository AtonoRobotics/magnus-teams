//! Talks to `magnus-computerd`, in the requests both define once
//! (`magnus_computerd_proto`): the socket, the op names, the version check
//! and the guest's reopening stay here. What a guest sends back is
//! untrusted, and stays a JSON value for the caller to read.

use anyhow::{anyhow, bail, Context, Result};
pub(crate) use magnus_computerd_proto::guest::{GuestRequest, MAX_GUEST_REPLY_BYTES};
pub(crate) use magnus_computerd_proto::Request;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Creating a computer makes its drives, a start waits for the VM, and a
/// hibernate or backup copies a whole disk or memory image.
const REPLY_WAIT: Duration = Duration::from_secs(600);
const GUEST_WAIT: Duration = Duration::from_secs(60);
const MAX_REPLY_BYTES: u64 = 1 << 16;
/// Computerd's words when a machine is at its limit of live computers.
const AT_CAPACITY: &str = "as many computers";

/// One request as it goes on the socket: a JSON line.
fn line(req: &impl serde::Serialize) -> Result<Vec<u8>> {
    let mut l = serde_json::to_vec(req)?;
    l.push(b'\n');
    Ok(l)
}

async fn read_line(s: impl tokio::io::AsyncRead + Unpin, limit: u64) -> Result<Value> {
    let mut line = String::new();
    BufReader::new(s.take(limit)).read_line(&mut line).await?;
    if !line.ends_with('\n') {
        bail!("the reply was cut short");
    }
    Ok(serde_json::from_str(&line)?)
}

fn checked(v: Value) -> Result<Value> {
    if v["ok"] == true {
        return Ok(v);
    }
    Err(anyhow!("{}", v["error"].as_str().unwrap_or("computerd refused")))
}

pub fn at_capacity(e: &anyhow::Error) -> bool {
    e.to_string().contains(AT_CAPACITY)
}

/// One request to computerd. One it is known not to serve is refused here,
/// and one it cannot read comes back in plain words: computerd is older
/// than this server (`service_version`).
pub async fn ask(socket: &Path, req: Request) -> Result<Value> {
    if super::service_version::known_to_lack(socket, req.op()).await {
        return Err(super::service_version::outdated());
    }
    ask_unchecked(socket, req).await.map_err(|e| if super::service_version::is_bad_request(&e) { super::service_version::outdated() } else { e })
}

/// The request as it is, and computerd's answer as it is.
pub(super) async fn ask_unchecked(socket: &Path, req: Request) -> Result<Value> {
    let reply = tokio::time::timeout(REPLY_WAIT, async {
        let mut s = UnixStream::connect(socket).await.context("reach magnus-computerd")?;
        s.write_all(&line(&req)?).await?;
        read_line(s, MAX_REPLY_BYTES).await
    });
    checked(reply.await.map_err(|_| anyhow!("magnus-computerd did not answer in time"))??)
}

/// A raw stream to a computer's desktop, after computerd says it is open.
pub async fn display(socket: &Path, id: &str) -> Result<UnixStream> {
    let mut s = UnixStream::connect(socket).await.context("reach magnus-computerd")?;
    s.write_all(&line(&Request::Display { id: id.to_string() })?).await?;
    let mut ack = Vec::new();
    loop {
        let byte = s.read_u8().await?;
        if byte == b'\n' {
            break;
        }
        ack.push(byte);
        anyhow::ensure!(ack.len() as u64 <= MAX_REPLY_BYTES, "the reply was too long");
    }
    checked(serde_json::from_slice(&ack)?)?;
    Ok(s)
}

/// One request to a computer's guest agent.
pub async fn guest(socket: &Path, id: &str, req: GuestRequest) -> Result<Value> {
    guest_for(socket, id, req, GUEST_WAIT).await
}

/// For work that takes as long as it takes, such as an install step.
pub async fn guest_for(socket: &Path, id: &str, req: GuestRequest, wait: Duration) -> Result<Value> {
    guest_line_for(socket, id, line(&req)?, wait).await
}

/// A request whose shape is the guest's own: a browser VM's, mostly the
/// model's tool arguments passed through.
pub async fn guest_value_for(socket: &Path, id: &str, req: &Value, wait: Duration) -> Result<Value> {
    guest_line_for(socket, id, line(req)?, wait).await
}

/// A connection that never reached the guest carried nothing, so it is
/// opened again, a few times: Firecracker drops a host's connection that a
/// guest busy just after boot has not taken within its two seconds. Once the
/// request is sent it is never sent again, whatever it was.
const GUEST_CONNECT_TRIES: u32 = 3;
const GUEST_CONNECT_RETRY_WAIT: Duration = Duration::from_millis(250);

type GuestReader = BufReader<tokio::io::Take<tokio::net::unix::OwnedReadHalf>>;

/// A stream to the guest agent, once computerd's relay says it reached it.
async fn open_guest(socket: &Path, id: &str) -> Result<(GuestReader, tokio::net::unix::OwnedWriteHalf)> {
    let s = UnixStream::connect(socket).await.context("reach magnus-computerd")?;
    let (r, mut w) = s.into_split();
    w.write_all(&line(&Request::Connect { id: id.to_string() })?).await?;
    let mut r = BufReader::new(r.take(MAX_GUEST_REPLY_BYTES));
    let mut ack = String::new();
    r.read_line(&mut ack).await?;
    checked(serde_json::from_str(&ack)?)?;
    Ok((r, w))
}

async fn open_guest_retrying(socket: &Path, id: &str) -> Result<(GuestReader, tokio::net::unix::OwnedWriteHalf)> {
    let mut tries = 1;
    loop {
        match open_guest(socket, id).await {
            Ok(opened) => return Ok(opened),
            Err(e) if tries >= GUEST_CONNECT_TRIES => return Err(e),
            Err(_) => {
                tries += 1;
                tokio::time::sleep(GUEST_CONNECT_RETRY_WAIT).await;
            }
        }
    }
}

async fn guest_line_for(socket: &Path, id: &str, req: Vec<u8>, wait: Duration) -> Result<Value> {
    let reply = tokio::time::timeout(wait, async {
        let (mut r, mut w) = open_guest_retrying(socket, id).await?;
        w.write_all(&req).await?;
        let mut line = String::new();
        r.read_line(&mut line).await?;
        anyhow::ensure!(line.ends_with('\n'), "the computer closed the connection");
        Ok::<Value, anyhow::Error>(serde_json::from_str(&line)?)
    });
    reply.await.map_err(|_| anyhow!("the computer did not answer in time"))?
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
