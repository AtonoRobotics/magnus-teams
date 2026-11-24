//! What the adapters' tests share (feature `testing`): a provider that
//! answers one HTTP request with a body given here, and one turn asked of
//! an adapter served in the test.

use crate::{serve, Call, CompleteParams, Event, Request, Sink};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// How long a test waits for the adapter's socket to appear.
const SOCKET_WAIT: Duration = Duration::from_secs(5);
const SOCKET_POLL: Duration = Duration::from_millis(10);

/// A provider that answers one request: the status line and headers
/// `head`, then `body`, then it hangs up. With `chunked`, the body is sent
/// as one chunk of a chunked body that never ends: the stream breaks.
pub async fn provider_once(head: &'static str, body: String, chunked: bool) -> anyhow::Result<SocketAddr> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        read_request(&mut stream).await?;
        let reply =
            if chunked { format!("{head}\r\ntransfer-encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n", body.len()) } else { format!("{head}\r\nconnection: close\r\n\r\n{body}") };
        stream.write_all(reply.as_bytes()).await?;
        stream.flush().await?;
        stream.shutdown().await?;
        anyhow::Ok(())
    });
    Ok(addr)
}

/// Read a whole request: its head, then the body its length names.
async fn read_request(stream: &mut tokio::net::TcpStream) -> anyhow::Result<()> {
    let mut got = vec![];
    let mut buf = [0u8; 8192];
    let end = loop {
        let n = stream.read(&mut buf).await?;
        anyhow::ensure!(n > 0, "the request ended early");
        got.extend_from_slice(&buf[..n]);
        if let Some(i) = got.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&got[..end]).to_ascii_lowercase();
    let length = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
    while got.len() < end + length {
        let n = stream.read(&mut buf).await?;
        anyhow::ensure!(n > 0, "the request body ended early");
        got.extend_from_slice(&buf[..n]);
    }
    Ok(())
}

/// Ask an adapter served here with `handle` for one turn, and gather what
/// it sends up to its `done` or `error`.
pub async fn one_turn<H, F>(handle: H, params: CompleteParams) -> anyhow::Result<Vec<Event>>
where
    H: Fn(Request, Sink) -> F + Clone + Send + Sync + 'static,
    F: std::future::Future<Output = anyhow::Result<()>> + Send,
{
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let socket = std::env::temp_dir().join(format!("magnus-adapter-test-{}-{}.sock", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
    let path = socket.clone();
    tokio::spawn(async move { serve(&path, handle).await });
    let deadline = tokio::time::Instant::now() + SOCKET_WAIT;
    while !socket.exists() {
        anyhow::ensure!(tokio::time::Instant::now() < deadline, "the adapter's socket did not appear");
        tokio::time::sleep(SOCKET_POLL).await;
    }
    let mut call = Call::open(&socket, Request::Complete(params)).await?;
    let mut out = vec![];
    while let Some(ev) = call.next().await? {
        let end = matches!(ev, Event::Done { .. } | Event::Error { .. });
        out.push(ev);
        if end {
            break;
        }
    }
    let _ = std::fs::remove_file(&socket);
    Ok(out)
}
