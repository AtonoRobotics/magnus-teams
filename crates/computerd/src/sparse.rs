//! A snapshot sealed chunk by chunk, with the memory the VM never touched
//! left out. A computer's memory file is as large as its memory (4 GiB),
//! and most of it is zeros. Sealing and opening every byte made a wake
//! slower than a cold boot: 10 s to open, once 198 s (owner, 2026-11-13:
//! "the computer is slow to load"). The spec stores snapshots "as chunked
//! copy-on-write". Here a chunk of zeros is kept as its tag alone and
//! opened as a hole, and the rest is opened in parallel.
//!
//! Each record: its kind (zero or data), the plain length (u32, little
//! endian), then the tag alone (zero) or the ciphertext with its tag
//! (data). The kind and length are authenticated with the chunk, and the
//! nonce carries its place and whether it is the last, so no chunk can be
//! dropped, moved, cut or turned into zeros unseen.

use anyhow::{bail, ensure, Result};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use std::fs::File;
use std::io::{BufRead, Read, Seek, SeekFrom, Write};

use crate::seal::KEY_BYTES;

/// What the snapshot's format file says for this format.
pub const FORMAT: &str = "sparse-1";
const CHUNK_BYTES: usize = 1 << 20;
const TAG_BYTES: usize = 16;
const HEADER_BYTES: usize = 5;
const ZERO: u8 = 0;
const DATA: u8 = 1;
/// How many chunks are opened at once, one per thread.
const BATCH_CHUNKS: usize = 16;

fn nonce(index: u64, last: bool) -> Nonce {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(&index.to_le_bytes());
    n[8] = u8::from(last);
    Nonce::from(n)
}

fn header(kind: u8, len: usize) -> Result<[u8; HEADER_BYTES]> {
    let len = u32::try_from(len)?;
    let mut h = [0u8; HEADER_BYTES];
    h[0] = kind;
    h[1..].copy_from_slice(&len.to_le_bytes());
    Ok(h)
}

/// Fills `buf`, unless the input ends first.
fn fill(from: &mut impl Read, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match from.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

pub fn seal(key: &[u8; KEY_BYTES], from: &mut impl Read, to: &mut impl Write) -> Result<()> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let (mut buf, mut next) = (vec![0u8; CHUNK_BYTES], vec![0u8; CHUNK_BYTES]);
    let mut filled = fill(from, &mut buf)?;
    let mut index = 0u64;
    loop {
        // Reading one ahead is how the last chunk knows it is the last.
        let more = fill(from, &mut next)?;
        let plain = &buf[..filled];
        let kind = if plain.iter().all(|b| *b == 0) { ZERO } else { DATA };
        let h = header(kind, filled)?;
        let msg: &[u8] = if kind == ZERO { &[] } else { plain };
        let sealed = cipher.encrypt(&nonce(index, more == 0), Payload { msg, aad: &h }).map_err(|_| anyhow::anyhow!("encrypt a snapshot"))?;
        to.write_all(&h)?;
        to.write_all(&sealed)?;
        if more == 0 {
            return Ok(());
        }
        std::mem::swap(&mut buf, &mut next);
        filled = more;
        index += 1;
    }
}

/// One record as read, before it is opened.
struct Record {
    index: u64,
    last: bool,
    header: [u8; HEADER_BYTES],
    sealed: Vec<u8>,
}

impl Record {
    fn len(&self) -> usize {
        u32::from_le_bytes([self.header[1], self.header[2], self.header[3], self.header[4]]) as usize
    }
}

fn read_record(from: &mut impl BufRead, index: u64) -> Result<Option<Record>> {
    let mut h = [0u8; HEADER_BYTES];
    match fill(from, &mut h)? {
        0 => return Ok(None),
        HEADER_BYTES => {}
        _ => bail!("a snapshot is cut short"),
    }
    let len = u32::from_le_bytes([h[1], h[2], h[3], h[4]]) as usize;
    ensure!(len <= CHUNK_BYTES, "a snapshot's chunk is larger than a chunk");
    let stored = match h[0] {
        ZERO => TAG_BYTES,
        DATA => len + TAG_BYTES,
        _ => bail!("a snapshot holds a chunk of no known kind"),
    };
    let mut sealed = vec![0u8; stored];
    ensure!(fill(from, &mut sealed)? == stored, "a snapshot is cut short");
    let last = from.fill_buf()?.is_empty();
    Ok(Some(Record { index, last, header: h, sealed }))
}

fn open_record(cipher: &ChaCha20Poly1305, r: &Record) -> Result<Option<Vec<u8>>> {
    let Ok(plain) = cipher.decrypt(&nonce(r.index, r.last), Payload { msg: &r.sealed, aad: &r.header }) else { bail!("a snapshot does not match its key") };
    Ok((r.header[0] == DATA).then_some(plain))
}

/// Opens a sparse snapshot into `to`: zeros as holes, the rest written.
pub fn open(key: &[u8; KEY_BYTES], from: &mut impl BufRead, to: &mut File) -> Result<()> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let (mut index, mut size, mut ended) = (0u64, 0u64, false);
    while !ended {
        let mut batch = Vec::with_capacity(BATCH_CHUNKS);
        while batch.len() < BATCH_CHUNKS {
            let Some(r) = read_record(from, index)? else { break };
            index += 1;
            ended = r.last;
            batch.push(r);
            if ended {
                break;
            }
        }
        ensure!(!batch.is_empty() || index > 0, "a snapshot is empty");
        ensure!(!batch.is_empty() || ended, "a snapshot is cut short");
        let opened: Vec<Result<Option<Vec<u8>>>> = std::thread::scope(|s| {
            let handles: Vec<_> = batch.iter().map(|r| s.spawn(|| open_record(&cipher, r))).collect();
            handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err(anyhow::anyhow!("opening a chunk failed")))).collect()
        });
        for (r, plain) in batch.iter().zip(opened) {
            match plain? {
                Some(p) => to.write_all(&p)?,
                None => {
                    to.seek(SeekFrom::Current(i64::try_from(r.len())?))?;
                }
            }
            size += r.len() as u64;
        }
    }
    // Zeros at the end are a hole too: the file keeps its whole size.
    to.set_len(size)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;
    use std::os::unix::fs::MetadataExt;

    const KEY: [u8; KEY_BYTES] = [7u8; KEY_BYTES];

    fn sealed(data: &[u8]) -> Vec<u8> {
        let mut out = vec![];
        seal(&KEY, &mut &data[..], &mut out).unwrap();
        out
    }

    fn opened(stored: &[u8]) -> Result<Vec<u8>> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mem");
        let mut f = File::options().read(true).write(true).create_new(true).open(&path).unwrap();
        open(&KEY, &mut BufReader::new(stored), &mut f)?;
        Ok(std::fs::read(&path).unwrap())
    }

    /// A memory file: zeros with a few chunks in use, and zeros at the end.
    fn memory(chunks: usize) -> Vec<u8> {
        let mut m = vec![0u8; chunks * CHUNK_BYTES + 77];
        for c in [1, 5, 6, BATCH_CHUNKS + 3] {
            if c < chunks {
                m[c * CHUNK_BYTES + 9] = 0xA5;
            }
        }
        m
    }

    #[test]
    fn the_bytes_come_back_and_zeros_are_not_stored() {
        for data in [vec![], vec![0u8; 3], vec![9u8; 3], vec![0u8; CHUNK_BYTES], memory(2), memory(BATCH_CHUNKS * 2 + 5)] {
            let s = sealed(&data);
            assert_eq!(opened(&s).unwrap(), data, "{} bytes did not come back", data.len());
        }
        let m = memory(BATCH_CHUNKS * 2 + 5);
        let used = 4 * (CHUNK_BYTES + TAG_BYTES);
        assert!(sealed(&m).len() < used + 64 * (HEADER_BYTES + TAG_BYTES), "the zeros were stored: {} bytes for {used} in use", sealed(&m).len());
    }

    #[test]
    fn untouched_memory_opens_as_holes() {
        let m = memory(64);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mem");
        let mut f = File::options().read(true).write(true).create_new(true).open(&path).unwrap();
        open(&KEY, &mut BufReader::new(&sealed(&m)[..]), &mut f).unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        assert_eq!(meta.len(), m.len() as u64);
        assert!(meta.blocks() * 512 < 16 * CHUNK_BYTES as u64, "the zeros were written: {} bytes on disk", meta.blocks() * 512);
    }

    #[test]
    fn a_changed_moved_cut_or_zeroed_chunk_is_refused() {
        let s = sealed(&memory(8));
        let mut flipped = s.clone();
        let at = s.len() / 2;
        flipped[at] ^= 1;
        assert!(opened(&flipped).is_err(), "a changed byte opened");
        let record = HEADER_BYTES + TAG_BYTES;
        assert!(opened(&s[..s.len() - record]).is_err(), "a snapshot cut at a chunk opened");
        assert!(opened(&s[..s.len() - 3]).is_err(), "a snapshot cut mid-chunk opened");
        // Chunk 1 holds data: stored as zeros instead, it must not open.
        let data_at = record;
        let mut zeroed = s[..data_at].to_vec();
        let mut z = sealed(&vec![0u8; CHUNK_BYTES * 2]);
        zeroed.extend(z.drain(record..2 * record));
        zeroed.extend(&s[data_at + HEADER_BYTES + CHUNK_BYTES + TAG_BYTES..]);
        assert!(opened(&zeroed).is_err(), "a chunk in use turned to zeros opened");
        let other = {
            let mut k = KEY;
            k[0] ^= 1;
            k
        };
        let dir = tempfile::tempdir().unwrap();
        let mut f = File::create(dir.path().join("m")).unwrap();
        assert!(open(&other, &mut BufReader::new(&s[..]), &mut f).is_err(), "another key opened it");
    }
}
