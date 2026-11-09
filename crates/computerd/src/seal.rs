//! Encrypts a snapshot chunk by chunk. A fresh key per snapshot, so a nonce
//! built from the chunk's number is never used twice with one key.

use anyhow::{bail, ensure, Context, Result};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use sha2::{Digest, Sha256};
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

/// A nonce from the key, the chunk's place and its bytes. A profile is sealed
/// again and again under one key, so a nonce from the place alone would repeat
/// over different bytes; this one repeats only over the same bytes, which also
/// leaves an unchanged chunk unchanged for the backups.
fn derived_nonce(key: &[u8; KEY_BYTES], index: u64, last: bool, plain: &[u8]) -> [u8; NONCE_BYTES] {
    let digest =
        Sha256::new().chain_update(b"magnus-profile-seal").chain_update(key).chain_update(index.to_le_bytes()).chain_update([u8::from(last)]).chain_update(plain).finalize();
    let mut nonce = [0u8; NONCE_BYTES];
    nonce.copy_from_slice(&digest[..NONCE_BYTES]);
    nonce
}

fn place(index: u64, last: bool) -> [u8; 9] {
    let mut aad = [0u8; 9];
    aad[..8].copy_from_slice(&index.to_le_bytes());
    aad[8] = u8::from(last);
    aad
}

/// For a key used more than once: a browser profile's.
pub fn seal_again(key: &[u8; KEY_BYTES], from: &mut impl Read, to: &mut impl Write) -> Result<()> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let (mut buf, mut next) = (vec![0u8; CHUNK_BYTES], vec![0u8; CHUNK_BYTES]);
    let mut filled = read_chunk(from, &mut buf)?;
    let mut index = 0u64;
    loop {
        let more = read_chunk(from, &mut next)?;
        let last = more == 0;
        let nonce = derived_nonce(key, index, last, &buf[..filled]);
        let sealed = cipher.encrypt(Nonce::from_slice(&nonce), Payload { msg: &buf[..filled], aad: &place(index, last) }).map_err(|_| anyhow::anyhow!("encrypt a profile"))?;
        to.write_all(&nonce)?;
        to.write_all(&sealed)?;
        if last {
            return Ok(());
        }
        std::mem::swap(&mut buf, &mut next);
        filled = more;
        index += 1;
    }
}

pub fn open_again(key: &[u8; KEY_BYTES], from: &mut impl Read, to: &mut impl Write) -> Result<()> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let whole = CHUNK_BYTES + TAG_BYTES + NONCE_BYTES;
    let (mut buf, mut next) = (vec![0u8; whole], vec![0u8; whole]);
    let mut filled = read_chunk(from, &mut buf)?;
    let mut index = 0u64;
    loop {
        let more = read_chunk(from, &mut next)?;
        let last = more == 0;
        ensure!(filled >= NONCE_BYTES + TAG_BYTES, "a profile is cut short");
        let (nonce, sealed) = buf[..filled].split_at(NONCE_BYTES);
        let Ok(plain) = cipher.decrypt(Nonce::from_slice(nonce), Payload { msg: sealed, aad: &place(index, last) }) else { bail!("a profile does not match its key") };
        to.write_all(&plain)?;
        if last {
            return Ok(());
        }
        std::mem::swap(&mut buf, &mut next);
        filled = more;
        index += 1;
    }
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

    fn resealed(data: &[u8], key: &[u8; KEY_BYTES]) -> Vec<u8> {
        let mut out = vec![];
        seal_again(key, &mut &data[..], &mut out).unwrap();
        out
    }

    #[test]
    fn a_profile_sealed_again_never_repeats_a_keystream_over_new_bytes() {
        let key = new_key().unwrap();
        let (a, b) = (vec![0x41u8; CHUNK_BYTES], vec![0x42u8; CHUNK_BYTES]);
        let (sa, sb) = (resealed(&a, &key), resealed(&b, &key));
        // The same key stream would make the two ciphertexts differ by exactly a ^ b.
        let same_stream = sa[NONCE_BYTES..NONCE_BYTES + 32].iter().zip(&sb[NONCE_BYTES..NONCE_BYTES + 32]).all(|(x, y)| x ^ y == 0x41 ^ 0x42);
        assert!(!same_stream, "two versions of a profile share a key stream");
        assert_ne!(sa[..NONCE_BYTES], sb[..NONCE_BYTES]);
    }

    #[test]
    fn a_profile_sealed_again_opens_and_keeps_unchanged_chunks() {
        let key = new_key().unwrap();
        for size in [0, 1, CHUNK_BYTES, CHUNK_BYTES * 2 + 5] {
            let data = vec![7u8; size];
            let mut back = vec![];
            open_again(&key, &mut &resealed(&data, &key)[..], &mut back).unwrap();
            assert_eq!(back, data, "size {size}");
        }
        let mut a = vec![1u8; CHUNK_BYTES * 3];
        let first = resealed(&a, &key);
        a[CHUNK_BYTES * 2] = 9;
        let second = resealed(&a, &key);
        let whole = CHUNK_BYTES + TAG_BYTES + NONCE_BYTES;
        assert_eq!(first[..whole * 2], second[..whole * 2], "an unchanged chunk came out different, so every backup is whole");
        assert_ne!(first[whole * 2..], second[whole * 2..]);
    }

    #[test]
    fn a_profile_refuses_a_wrong_key_a_change_a_cut_or_a_swap() {
        let key = new_key().unwrap();
        let s = resealed(&(0..CHUNK_BYTES * 2 + 5).map(|i| i as u8).collect::<Vec<_>>(), &key);
        assert!(open_again(&new_key().unwrap(), &mut &s[..], &mut vec![]).is_err());
        let mut changed = s.clone();
        changed[100] ^= 1;
        assert!(open_again(&key, &mut &changed[..], &mut vec![]).is_err());
        let whole = CHUNK_BYTES + TAG_BYTES + NONCE_BYTES;
        assert!(open_again(&key, &mut &s[..whole], &mut vec![]).is_err(), "losing the end went unnoticed");
        let mut swapped = s[whole..whole * 2].to_vec();
        swapped.extend_from_slice(&s[..whole]);
        swapped.extend_from_slice(&s[whole * 2..]);
        assert!(open_again(&key, &mut &swapped[..], &mut vec![]).is_err(), "chunks moved went unnoticed");
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
