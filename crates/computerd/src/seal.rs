//! Encrypts a snapshot chunk by chunk. A fresh key per snapshot, so a nonce
//! built from the chunk's number is never used twice with one key.

use anyhow::{bail, ensure, Context, Result};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use std::io::{Read, Write};

pub const KEY_BYTES: usize = 32;
const CHUNK_BYTES: usize = 1 << 20;
const TAG_BYTES: usize = 16;
const NONCE_BYTES: usize = 12;

fn nonce(index: u64, last: bool) -> Nonce {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(&index.to_le_bytes());
    n[8] = u8::from(last);
    Nonce::from(n)
}

pub fn new_key() -> Result<[u8; KEY_BYTES]> {
    let mut key = [0u8; KEY_BYTES];
    std::fs::File::open("/dev/urandom").context("open the system's randomness")?.read_exact(&mut key)?;
    Ok(key)
}

/// One chunk of a backup: a random nonce, so the same bytes stored twice under
/// one key never share one, then the ciphertext.
pub fn seal_chunk(key: &[u8; KEY_BYTES], plain: &[u8]) -> Result<Vec<u8>> {
    let mut nonce = [0u8; NONCE_BYTES];
    std::fs::File::open("/dev/urandom").context("open the system's randomness")?.read_exact(&mut nonce)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let sealed = cipher.encrypt(Nonce::from_slice(&nonce), plain).map_err(|_| anyhow::anyhow!("encrypt a backup"))?;
    Ok([&nonce[..], &sealed].concat())
}

pub fn open_chunk(key: &[u8; KEY_BYTES], stored: &[u8]) -> Result<Vec<u8>> {
    ensure!(stored.len() >= NONCE_BYTES + TAG_BYTES, "a backup chunk is cut short");
    let (nonce, sealed) = stored.split_at(NONCE_BYTES);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    cipher.decrypt(Nonce::from_slice(nonce), sealed).map_err(|_| anyhow::anyhow!("a backup chunk does not match its key"))
}

/// Fills `buf`, unless the input ends first.
fn read_chunk(from: &mut impl Read, buf: &mut [u8]) -> Result<usize> {
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
    let mut filled = read_chunk(from, &mut buf)?;
    let mut index = 0u64;
    loop {
        // Reading one ahead is how the last chunk knows it is the last.
        let more = read_chunk(from, &mut next)?;
        let sealed = cipher.encrypt(&nonce(index, more == 0), &buf[..filled]).map_err(|_| anyhow::anyhow!("encrypt a snapshot"))?;
        to.write_all(&sealed)?;
        if more == 0 {
            return Ok(());
        }
        std::mem::swap(&mut buf, &mut next);
        filled = more;
        index += 1;
    }
}

pub fn open(key: &[u8; KEY_BYTES], from: &mut impl Read, to: &mut impl Write) -> Result<()> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let (mut buf, mut next) = (vec![0u8; CHUNK_BYTES + TAG_BYTES], vec![0u8; CHUNK_BYTES + TAG_BYTES]);
    let mut filled = read_chunk(from, &mut buf)?;
    let mut index = 0u64;
    loop {
        let more = read_chunk(from, &mut next)?;
        ensure!(filled >= TAG_BYTES, "a snapshot is cut short");
        let Ok(plain) = cipher.decrypt(&nonce(index, more == 0), &buf[..filled]) else { bail!("a snapshot does not match its key") };
        to.write_all(&plain)?;
        if more == 0 {
            return Ok(());
        }
        std::mem::swap(&mut buf, &mut next);
        filled = more;
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sealed(data: &[u8], key: &[u8; KEY_BYTES]) -> Vec<u8> {
        let mut out = vec![];
        seal(key, &mut &data[..], &mut out).unwrap();
        out
    }

    #[test]
    fn what_is_sealed_opens_and_is_not_readable() {
        let key = new_key().unwrap();
        for size in [0, 1, CHUNK_BYTES - 1, CHUNK_BYTES, CHUNK_BYTES + 1, CHUNK_BYTES * 3] {
            let data = vec![0x41u8; size];
            let s = sealed(&data, &key);
            if size > 32 {
                assert!(!s.windows(32).any(|w| w == &data[..32]), "plaintext is visible at size {size}");
            }
            let mut back = vec![];
            open(&key, &mut &s[..], &mut back).unwrap();
            assert_eq!(back, data, "size {size}");
        }
    }

    #[test]
    fn a_wrong_key_a_change_or_a_cut_is_refused() {
        let key = new_key().unwrap();
        let s = sealed(&vec![7u8; CHUNK_BYTES * 2 + 5], &key);
        assert!(open(&new_key().unwrap(), &mut &s[..], &mut vec![]).is_err(), "a wrong key opened it");
        let mut changed = s.clone();
        changed[100] ^= 1;
        assert!(open(&key, &mut &changed[..], &mut vec![]).is_err(), "a change went unnoticed");
        let whole_chunks = CHUNK_BYTES + TAG_BYTES;
        assert!(open(&key, &mut &s[..whole_chunks], &mut vec![]).is_err(), "losing the end went unnoticed");
    }

    #[test]
    fn a_chunk_opens_only_whole_and_with_its_key() {
        let key = new_key().unwrap();
        let a = seal_chunk(&key, b"same bytes").unwrap();
        assert_ne!(a, seal_chunk(&key, b"same bytes").unwrap(), "a nonce was reused");
        assert_eq!(open_chunk(&key, &a).unwrap(), b"same bytes");
        let mut changed = a.clone();
        changed[20] ^= 1;
        assert!(open_chunk(&key, &changed).is_err());
        assert!(open_chunk(&new_key().unwrap(), &a).is_err());
    }
}
