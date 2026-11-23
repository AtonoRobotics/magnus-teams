//! One way to reach a computer's files: its live guest, or for a stopped
//! one a helper VM with its volume and no network, started once for the
//! whole of a use. The host never mounts a volume. A file is read in pieces,
//! so no one answer carries all of it and the guest never holds it whole,
//! with one ceiling for every reader
//! (`decisions/2026-11-23-one-check-for-using-a-computer.md`).

use super::client::{self, GuestRequest, Request};
use super::service::{failed, wait_for_guest};
use super::{Computer, Status};
use crate::controls::Refused;
use crate::kernel::Kernel;
use base64::Engine;
use serde_json::{json, Value};
use std::future::Future;
use std::path::PathBuf;

/// The most one file read carries, in bytes: a download, a takeover's
/// read, and what an export holds in all.
pub(crate) const MAX_FILE_BYTES: usize = 128 * 1024 * 1024;
/// Read in pieces this size, in bytes.
const READ_PIECE_BYTES: u64 = 4 * 1024 * 1024;

/// A stopped computer's files are reached one helper VM at a time.
static HELPER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A computer's files, reachable for as long as one use lasts.
#[derive(Clone)]
pub(crate) struct Files {
    socket: PathBuf,
    id: String,
}

impl Files {
    /// One request to the guest. Its own error comes back as a refusal.
    pub(crate) async fn ask(&self, req: GuestRequest) -> Result<Value, Refused> {
        let reply = client::guest(&self.socket, &self.id, req).await.map_err(failed)?;
        match reply["error"].as_str() {
            Some(e) => Err(Refused::Failed(e.to_string())),
            None => Ok(reply),
        }
    }

    /// A whole file, in pieces, refused once it passes `limit` bytes.
    pub(crate) async fn read_whole(&self, path: &str, limit: usize) -> Result<Vec<u8>, Refused> {
        read_in_pieces(path, limit, |req| self.ask(req)).await
    }
}

/// A whole file through `ask` (the guest's ranged `read`), in pieces,
/// refused once it passes `limit` bytes.
pub(crate) async fn read_in_pieces<F, Fut>(path: &str, limit: usize, mut ask: F) -> Result<Vec<u8>, Refused>
where
    F: FnMut(GuestRequest) -> Fut,
    Fut: Future<Output = Result<Value, Refused>>,
{
    let mut out = vec![];
    loop {
        let v = ask(GuestRequest::Read { path: path.to_string(), offset: Some(out.len() as u64), length: Some(READ_PIECE_BYTES) }).await?;
        let piece = base64::engine::general_purpose::STANDARD.decode(v["data_b64"].as_str().unwrap_or_default()).map_err(|e| Refused::Failed(format!("{path}: {e}")))?;
        let short = (piece.len() as u64) < READ_PIECE_BYTES;
        out.extend(piece);
        if out.len() > limit {
            return Err(Refused::Invalid(format!("{path} is larger than {} MB", limit / 1024 / 1024)));
        }
        if short {
            return Ok(out);
        }
    }
}

/// A whole file as a read answers it.
pub(super) fn as_reply(path: &str, bytes: &[u8]) -> Value {
    json!({ "path": path, "data_b64": base64::engine::general_purpose::STANDARD.encode(bytes) })
}

impl Kernel {
    /// Runs `work` on `c`'s files, on a computer the caller may already
    /// reach. A stopped one's helper starts once for all of it, read-only
    /// when `read_only` (which keeps a hibernated computer's memory snapshot
    /// usable), and its writes are flushed before it goes.
    pub(crate) async fn with_files<T, F, Fut>(&self, c: &Computer, read_only: bool, work: F) -> Result<T, Refused>
    where
        F: FnOnce(Files) -> Fut,
        Fut: Future<Output = Result<T, Refused>>,
    {
        let socket = self.computer_socket()?;
        let files = Files { socket: socket.clone(), id: c.id.clone() };
        match c.status {
            Status::Running => {
                let done = work(files.clone()).await;
                files.ask(GuestRequest::Sync).await?;
                done
            }
            Status::Stopped | Status::Hibernated => {
                let _one_at_a_time = HELPER.lock().await;
                client::ask(&socket, Request::StartHelper { id: c.id.clone(), read_only }).await.map_err(failed)?;
                let done = async {
                    wait_for_guest(&socket, &c.id).await.map_err(failed)?;
                    let done = work(files.clone()).await;
                    files.ask(GuestRequest::Sync).await?;
                    done
                }
                .await;
                client::ask(&socket, Request::Stop { id: c.id.clone() }).await.map_err(failed)?;
                done
            }
            Status::Queued | Status::Paused | Status::Killed => Err(Refused::Denied(format!("the computer is {}", json!(c.status).as_str().unwrap_or("not running")))),
        }
    }

    /// One file request on a computer the caller may already reach.
    pub(crate) async fn computer_file_for(&self, c: &Computer, req: GuestRequest) -> Result<Value, Refused> {
        let read_only = matches!(req, GuestRequest::Read { .. } | GuestRequest::List { .. });
        self.with_files(c, read_only, |f| async move { f.ask(req).await }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A guest holding one file, answering ranged reads as the real one does.
    async fn guest(file: &[u8], req: GuestRequest, asked: &std::sync::Mutex<Vec<(u64, u64)>>) -> Result<Value, Refused> {
        let GuestRequest::Read { offset, length, .. } = req else { return Err(Refused::Failed("unknown op".into())) };
        let (Some(offset), Some(length)) = (offset, length) else { return Err(Refused::Failed("a whole file was asked for in one answer".into())) };
        asked.lock().unwrap().push((offset, length));
        let start = (offset as usize).min(file.len());
        let end = (start + length as usize).min(file.len());
        Ok(json!({ "data_b64": base64::engine::general_purpose::STANDARD.encode(&file[start..end]) }))
    }

    #[tokio::test]
    async fn a_file_larger_than_one_answer_is_read_whole_in_pieces_up_to_one_ceiling() {
        let big: Vec<u8> = (0..(READ_PIECE_BYTES as usize * 2 + 5)).map(|i| (i % 251) as u8).collect();
        let asked = std::sync::Mutex::new(vec![]);
        let got = read_in_pieces("/workspace/big.bin", MAX_FILE_BYTES, |req| guest(&big, req, &asked)).await.unwrap();
        assert!(got == big, "the pieces did not make the file");
        assert_eq!(asked.lock().unwrap().len(), 3);
        assert!(asked.lock().unwrap().iter().all(|(_, length)| *length == READ_PIECE_BYTES));
        let over = read_in_pieces("/workspace/big.bin", READ_PIECE_BYTES as usize, |req| guest(&big, req, &asked)).await;
        assert!(matches!(over, Err(Refused::Invalid(_))), "a file over the ceiling was read");
        let exact: Vec<u8> = vec![7; READ_PIECE_BYTES as usize];
        assert!(read_in_pieces("/workspace/e.bin", MAX_FILE_BYTES, |req| guest(&exact, req, &asked)).await.unwrap() == exact);
    }
}
