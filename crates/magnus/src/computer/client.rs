//! Talks to `magnus-computerd`. What a guest sends back is untrusted.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Creating a computer makes its drives, a start waits for the VM, and a
/// hibernate or backup copies a whole disk or memory image.
const REPLY_WAIT: Duration = Duration::from_secs(600);
const GUEST_WAIT: Duration = Duration::from_secs(60);
const MAX_REPLY_BYTES: u64 = 1 << 16;
/// A file read comes back whole, in base64.
const MAX_GUEST_REPLY_BYTES: u64 = 64 << 20;
/// Computerd's words when a machine is at its limit of live computers.
const AT_CAPACITY: &str = "as many computers";

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
pub async fn ask(socket: &Path, req: Value) -> Result<Value> {
    let op = req["op"].as_str().unwrap_or_default().to_string();
    if super::service_version::known_to_lack(socket, &op).await {
        return Err(super::service_version::outdated());
    }
    ask_unchecked(socket, req).await.map_err(|e| if super::service_version::is_bad_request(&e) { super::service_version::outdated() } else { e })
}

/// The request as it is, and computerd's answer as it is.
pub(super) async fn ask_unchecked(socket: &Path, req: Value) -> Result<Value> {
    let reply = tokio::time::timeout(REPLY_WAIT, async {
        let mut s = UnixStream::connect(socket).await.context("reach magnus-computerd")?;
        s.write_all(format!("{req}\n").as_bytes()).await?;
        read_line(s, MAX_REPLY_BYTES).await
    });
    checked(reply.await.map_err(|_| anyhow!("magnus-computerd did not answer in time"))??)
}

/// A raw stream to a computer's desktop, after computerd says it is open.
pub async fn display(socket: &Path, id: &str) -> Result<UnixStream> {
    let mut s = UnixStream::connect(socket).await.context("reach magnus-computerd")?;
    s.write_all(format!("{}\n", json!({ "op": "display", "id": id })).as_bytes()).await?;
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
pub async fn guest(socket: &Path, id: &str, req: Value) -> Result<Value> {
    guest_for(socket, id, req, GUEST_WAIT).await
}

/// For work that takes as long as it takes, such as an install step.
pub async fn guest_for(socket: &Path, id: &str, req: Value, wait: Duration) -> Result<Value> {
    let reply = tokio::time::timeout(wait, async {
        let s = UnixStream::connect(socket).await.context("reach magnus-computerd")?;
        let (r, mut w) = s.into_split();
        w.write_all(format!("{}\n", json!({ "op": "connect", "id": id })).as_bytes()).await?;
        let mut r = BufReader::new(r.take(MAX_GUEST_REPLY_BYTES));
        let mut ack = String::new();
        r.read_line(&mut ack).await?;
        checked(serde_json::from_str(&ack)?)?;
        w.write_all(format!("{req}\n").as_bytes()).await?;
        let mut line = String::new();
        r.read_line(&mut line).await?;
        anyhow::ensure!(line.ends_with('\n'), "the computer closed the connection");
        Ok::<Value, anyhow::Error>(serde_json::from_str(&line)?)
    });
    reply.await.map_err(|_| anyhow!("the computer did not answer in time"))?
}
