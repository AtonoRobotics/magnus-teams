//! Time-based one-time codes (RFC 6238, SHA-1): the second factor for
//! local password sign-in (#429). Secrets are base32 (RFC 4648), as
//! authenticator apps enroll them; the HMAC is ring's.

use chrono::{DateTime, Utc};

/// Seconds per code step.
pub const STEP: i64 = 30;

/// Digits in a code the appliance asks for.
pub const DIGITS: u32 = 6;

/// Steps of clock skew accepted each way.
const WINDOW: i64 = 1;

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for &b in bytes {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((acc << (5 - bits)) & 31) as usize] as char);
    }
    while !out.len().is_multiple_of(8) {
        out.push('=');
    }
    out
}

fn sextet(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some((c - b'A') as u32),
        b'a'..=b'z' => Some((c - b'a') as u32),
        b'2'..=b'7' => Some((c - b'2' + 26) as u32),
        _ => None,
    }
}

pub fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    if s.is_empty() {
        return Some(vec![]);
    }
    let mut out = Vec::with_capacity(s.len() * 5 / 8);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for &c in s.as_bytes() {
        acc = (acc << 5) | sextet(c)?;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The code for `secret` at `counter` (a 30-second step count), decimal
/// `digits` long.
pub fn generate(secret: &[u8], counter: u64, digits: u32) -> String {
    // SHA-1 because the authenticator protocol is SHA-1 (RFC 6238): every
    // app enrolls SHA-1 secrets, so nothing else verifies them.
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
    let mac = ring::hmac::sign(&key, &counter.to_be_bytes());
    let h = mac.as_ref();
    let offset = (h[h.len() - 1] & 0x0f) as usize;
    let code = u32::from_be_bytes([h[offset] & 0x7f, h[offset + 1], h[offset + 2], h[offset + 3]]) % 10u32.pow(digits);
    format!("{code:0digits$}", digits = digits as usize)
}

/// Whether `code` is the current code for the base32 `secret` at `at`,
/// accepting one step of skew each way.
pub fn verify(secret_b32: &str, code: &str, at: DateTime<Utc>) -> bool {
    verify_step(secret_b32, code, at).is_some()
}

/// The step `code` matches for the base32 `secret` at `at`, lowest first,
/// so a code always reports its own step while it is live (#441).
pub fn verify_step(secret_b32: &str, code: &str, at: DateTime<Utc>) -> Option<i64> {
    let code = code.trim();
    let secret = base32_decode(secret_b32)?;
    let step = at.timestamp().div_euclid(STEP);
    for d in -WINDOW..=WINDOW {
        let Some(counter) = step.checked_add(d).and_then(|s| u64::try_from(s).ok()) else { continue };
        let want = generate(&secret, counter, DIGITS);
        if constant_eq(code.as_bytes(), want.as_bytes()) {
            return step.checked_add(d);
        }
    }
    None
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_matches_rfc_4648() {
        for (raw, b32) in [("", ""), ("f", "MY======"), ("fo", "MZXQ===="), ("foo", "MZXW6==="), ("foob", "MZXW6YQ="), ("fooba", "MZXW6YTB"), ("foobar", "MZXW6YTBOI======")] {
            assert_eq!(base32_encode(raw.as_bytes()), b32, "encode {raw:?}");
            assert_eq!(base32_decode(b32).as_deref(), Some(raw.as_bytes()), "decode {b32}");
        }
        assert_eq!(base32_decode("mzxw6ytboi======").as_deref(), Some(b"foobar".as_slice()), "lowercase decodes");
        assert!(base32_decode("!!!!").is_none(), "junk decodes");
    }

    /// RFC 6238 appendix B, SHA-1 (8 digits, secret "12345678901234567890").
    #[test]
    fn totp_matches_rfc_6238() {
        let secret = b"12345678901234567890";
        for (t, code) in [(59i64, "94287082"), (1111111109, "07081804"), (1111111111, "14050471"), (1234567890, "89005924"), (2000000000, "69279037"), (20000000000, "65353130")] {
            assert_eq!(generate(secret, (t / 30) as u64, 8), code, "at T={t}");
        }
    }

    /// The matched step comes back for replay tracking: a code for step N
    /// reports N even when the clock has moved a step on (#441).
    #[test]
    fn verify_step_reports_the_matched_step() {
        let secret = base32_encode(b"12345678901234567890");
        let at = DateTime::from_timestamp(1234567890, 0).unwrap();
        let step = 1234567890i64.div_euclid(30);
        let code = generate(b"12345678901234567890", step as u64, 6);
        assert_eq!(verify_step(&secret, &code, at), Some(step));
        assert_eq!(verify_step(&secret, &code, at + chrono::Duration::seconds(30)), Some(step), "a step on, the code still reports its own step");
        assert_eq!(verify_step(&secret, "000000", at), None, "a wrong code reported a step");
    }

    #[test]
    fn verify_accepts_a_step_of_skew_and_nothing_more() {
        let secret = base32_encode(b"12345678901234567890");
        let at = DateTime::from_timestamp(1234567890, 0).unwrap();
        let now = generate(b"12345678901234567890", (1234567890 / 30) as u64, 6);
        assert!(verify(&secret, &now, at), "the current code refused");
        assert!(verify(&secret, &now, at + chrono::Duration::seconds(30)), "one step past refused");
        assert!(verify(&secret, &now, at - chrono::Duration::seconds(30)), "one step before refused");
        assert!(!verify(&secret, &now, at + chrono::Duration::seconds(61)), "two steps past accepted");
        assert!(!verify(&secret, &now, at - chrono::Duration::seconds(61)), "two steps before accepted");
        assert!(!verify(&secret, "000000", at), "a wrong code accepted");
        assert!(!verify("!!!!", &now, at), "a junk secret accepted");
    }
}
