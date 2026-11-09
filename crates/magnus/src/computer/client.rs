//! Talks to `magnus-computerd`. What a guest sends back is untrusted.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Creating a computer makes its drives, and a start waits for the VM.
const REPLY_WAIT: Duration = Duration::from_secs(120);
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

pub async fn ask(socket: &Path, req: Value) -> Result<Value> {
    let reply = tokio::time::timeout(REPLY_WAIT, async {
        let mut s = UnixStream::connect(socket).await.context("reach magnus-computerd")?;
        s.write_all(format!("{req}\n").as_bytes()).await?;
        read_line(s, MAX_REPLY_BYTES).await
    });
    checked(reply.await.map_err(|_| anyhow!("magnus-computerd did not answer in time"))??)
}

/// One request to a computer's guest agent.
pub async fn guest(socket: &Path, id: &str, req: Value) -> Result<Value> {
    let reply = tokio::time::timeout(GUEST_WAIT, async {
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
