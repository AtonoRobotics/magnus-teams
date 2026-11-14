//! A takeover's screen: bytes between computerd's display stream and the
//! person's WebSocket (spec, User takeover and handback).
//!
//! The guest's VNC server sends raw pixels, 4 MB for each full 1280x800
//! frame, and the client draws raw pixels only. A client that offers
//! `magnus.display.deflate` gets the screen's bytes through one deflate
//! stream, flushed at every chunk so it can draw at once: a desktop's pixels
//! shrink many times over. One that does not, an older app among them, gets
//! them as they are (`decisions/2026-11-11-a-faster-screen.md`).

use super::takeover::DISPLAY_PROTOCOL;
use anyhow::Result;
use axum::extract::ws::{Message, WebSocket};
use flate2::{Compress, Compression, FlushCompress};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::watch;

pub(super) const DEFLATE_PROTOCOL: &str = "magnus.display.deflate";
const DISPLAY_BUFFER_BYTES: usize = 64 * 1024;
/// Room a deflate call is given past what it has written, so a full buffer
/// is never taken for a finished flush.
const DEFLATE_SPARE_BYTES: usize = 4096;
/// RFB's FramebufferUpdateRequest: type 3, ten bytes. The first one a client
/// sends asks for the first frame.
const UPDATE_REQUEST: u8 = 3;
const UPDATE_REQUEST_BYTES: usize = 10;
const BYTES_PER_KIB: u64 = 1024;

/// The subprotocols the server takes, best first.
pub(super) const PROTOCOLS: [&str; 2] = [DEFLATE_PROTOCOL, DISPLAY_PROTOCOL];

/// Each computer's open screens hear when a handback cuts them, so no one
/// acts on the computer once its cleanup has begun (computer/leftovers.rs).
static CUTS: Mutex<Option<HashMap<String, watch::Sender<u64>>>> = Mutex::new(None);

/// Taken before the stream is opened: a cut after this is never missed.
pub(super) fn cut_signal(id: &str) -> watch::Receiver<u64> {
    let mut slot = CUTS.lock().unwrap_or_else(|e| e.into_inner());
    slot.get_or_insert_with(HashMap::new).entry(id.to_string()).or_insert_with(|| watch::channel(0).0).subscribe()
}

/// Closes every screen open on `id`.
pub(super) fn cut(id: &str) {
    if let Some(tx) = CUTS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(id)) {
        tx.send_modify(|n| *n += 1);
    }
}

/// Whether the client offered the deflated screen.
pub(super) fn wants_deflate(offered: &str) -> bool {
    offered.split(',').map(str::trim).any(|p| p == DEFLATE_PROTOCOL)
}

/// One zlib stream over the whole connection.
pub(super) struct Squeeze(Compress);

impl Squeeze {
    pub(super) fn new() -> Squeeze {
        // Fast: the screen is waiting. The zlib header is what a browser's
        // DecompressionStream("deflate") reads.
        Squeeze(Compress::new(Compression::fast(), true))
    }

    /// `input`, deflated and flushed, so everything in it can be inflated
    /// from what has been sent so far.
    pub(super) fn chunk(&mut self, input: &[u8]) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(input.len() / 2 + DEFLATE_SPARE_BYTES);
        let mut taken = 0;
        loop {
            if out.capacity() - out.len() < DEFLATE_SPARE_BYTES {
                out.reserve(out.capacity().max(DEFLATE_SPARE_BYTES));
            }
            let before = self.0.total_in();
            self.0.compress_vec(&input[taken..], &mut out, FlushCompress::Sync)?;
            taken += usize::try_from(self.0.total_in() - before)?;
            // Done when all is taken and the flush did not fill the buffer.
            if taken == input.len() && out.len() < out.capacity() {
                return Ok(out);
            }
        }
    }
}

/// What one takeover's stream did, for the log.
#[derive(Default)]
struct Tally {
    asked: Option<Instant>,
    first_frame: Option<Duration>,
    from_screen: u64,
    sent: u64,
}

/// Bytes both ways until either side stops.
pub(super) async fn carry(mut socket: WebSocket, mut stream: UnixStream, deflate: bool, id: String, mut cut: watch::Receiver<u64>) {
    let opened = Instant::now();
    let mut squeeze = deflate.then(Squeeze::new);
    let mut tally = Tally::default();
    let mut buf = vec![0u8; DISPLAY_BUFFER_BYTES];
    loop {
        tokio::select! {
            _ = cut.changed() => break,
            from_screen = stream.read(&mut buf) => match from_screen {
                Ok(n) if n > 0 => {
                    if let (None, Some(asked)) = (tally.first_frame, tally.asked) {
                        tally.first_frame = Some(asked.elapsed());
                    }
                    tally.from_screen += n as u64;
                    let out = match squeeze.as_mut().map(|z| z.chunk(&buf[..n])) {
                        Some(Ok(z)) => z,
                        Some(Err(_)) => break,
                        None => buf[..n].to_vec(),
                    };
                    tally.sent += out.len() as u64;
                    if socket.send(Message::Binary(out.into())).await.is_err() {
                        break;
                    }
                }
                _ => break,
            },
            from_person = socket.recv() => match from_person {
                Some(Ok(Message::Binary(b))) => {
                    if tally.asked.is_none() && b.len() == UPDATE_REQUEST_BYTES && b[0] == UPDATE_REQUEST {
                        tally.asked = Some(Instant::now());
                    }
                    if stream.write_all(&b).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Text(_))) => {}
                _ => break,
            },
        }
    }
    let first = tally.first_frame.map_or_else(|| "never began".to_string(), |d| format!("began {} ms after it was asked for", d.as_millis()));
    tracing::info!(
        "computer {id}'s screen: open {} s; the first frame {first}; {} KiB from the screen sent as {} KiB{}",
        opened.elapsed().as_secs(),
        tally.from_screen / BYTES_PER_KIB,
        tally.sent / BYTES_PER_KIB,
        if deflate { " deflated" } else { "" }
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Decompress, FlushDecompress};

    #[tokio::test]
    async fn a_handback_cuts_every_screen_open_on_its_computer_and_no_other() {
        let mut mine = cut_signal("cut-a");
        let mut also_mine = cut_signal("cut-a");
        let other = cut_signal("cut-b");
        cut("cut-a");
        assert!(mine.changed().await.is_ok() && also_mine.changed().await.is_ok());
        assert!(!other.has_changed().unwrap(), "another computer's screen was cut");
        cut("never-opened");
    }

    #[test]
    fn only_a_client_that_offers_it_gets_the_screen_deflated() {
        assert!(wants_deflate("magnus.ticket.abc, magnus.display.deflate, magnus.display"));
        assert!(!wants_deflate("magnus.ticket.abc, magnus.display"), "an older app would get bytes it cannot read");
        assert_eq!(PROTOCOLS[0], DEFLATE_PROTOCOL, "a client offering both got the raw screen");
    }

    #[test]
    fn each_chunk_can_be_drawn_as_soon_as_it_arrives_and_a_desktop_shrinks() {
        // A flat desktop with a little detail: what a screen mostly is.
        let frame: Vec<u8> = (0..DISPLAY_BUFFER_BYTES).map(|i| if i % 4096 < 64 { (i % 251) as u8 } else { 0x2a }).collect();
        let mut z = Squeeze::new();
        let mut inflate = Decompress::new(true);
        let mut sent = 0;
        for _ in 0..3 {
            let squeezed = z.chunk(&frame).unwrap();
            sent += squeezed.len();
            // Only what has been sent so far: the whole chunk comes out.
            let mut out = Vec::with_capacity(frame.len() + DEFLATE_SPARE_BYTES);
            inflate.decompress_vec(&squeezed, &mut out, FlushDecompress::Sync).unwrap();
            assert_eq!(out, frame, "a chunk could not be drawn until more came");
        }
        assert!(sent * 10 < frame.len() * 3, "the screen was not made smaller: {sent} bytes for {}", frame.len() * 3);
    }
}
