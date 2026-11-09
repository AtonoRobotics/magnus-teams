//! When a certificate expires, read from its PEM file, for Overview's alert
//! ("a certificate expires within 14 days") and Operations › Network. Only
//! the first certificate in a file counts: the one a server presents.

use base64::Engine;
use chrono::{DateTime, NaiveDateTime, Utc};
use std::path::Path;

/// One DER element: its tag, its contents, and what follows it.
fn tlv(d: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = d.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = if first & 0x80 == 0 {
        (first as usize, rest)
    } else {
        let n = (first & 0x7f) as usize;
        if n == 0 || n > 4 || rest.len() < n {
            return None;
        }
        (rest[..n].iter().fold(0usize, |a, b| (a << 8) | *b as usize), &rest[n..])
    };
    (rest.len() >= len).then(|| (tag, &rest[..len], &rest[len..]))
}

fn time(tag: u8, v: &[u8]) -> Option<DateTime<Utc>> {
    let s = std::str::from_utf8(v).ok()?.strip_suffix('Z')?;
    let full = match tag {
        // UTCTime: two-digit years, 1950-2049 (RFC 5280).
        0x17 => format!("{}{s}", if s.get(..2)?.parse::<u8>().ok()? < 50 { "20" } else { "19" }),
        0x18 => s.to_string(),
        _ => return None,
    };
    NaiveDateTime::parse_from_str(&full, "%Y%m%d%H%M%S").ok().map(|t| t.and_utc())
}

/// The end of a DER certificate's validity.
pub fn not_after_der(der: &[u8]) -> Option<DateTime<Utc>> {
    let (_, cert, _) = tlv(der)?;
    let (_, tbs, _) = tlv(cert)?;
    let mut rest = tbs;
    // The version is optional, tagged [0].
    let (tag, _, after) = tlv(rest)?;
    if tag == 0xa0 {
        rest = after;
    }
    // Serial number, signature algorithm, issuer, then validity.
    for _ in 0..3 {
        rest = tlv(rest)?.2;
    }
    let (_, validity, _) = tlv(rest)?;
    let (_, _, after_start) = tlv(validity)?;
    let (tag, end, _) = tlv(after_start)?;
    time(tag, end)
}

/// The first certificate in a PEM file, as DER.
pub fn first_der(pem: &str) -> Option<Vec<u8>> {
    let start = pem.find("-----BEGIN CERTIFICATE-----")? + "-----BEGIN CERTIFICATE-----".len();
    let end = start + pem[start..].find("-----END CERTIFICATE-----")?;
    let b64: String = pem[start..end].chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD.decode(b64).ok()
}

pub fn not_after(path: &Path) -> Option<DateTime<Utc>> {
    not_after_der(&first_der(&std::fs::read_to_string(path).ok()?)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_end_of_a_certificates_validity_is_read_from_its_pem() {
        let mut p = rcgen::CertificateParams::new(vec!["admin.acme.test".into()]).unwrap();
        p.not_after = rcgen::date_time_ymd(2031, 7, 4);
        let key = rcgen::KeyPair::generate().unwrap();
        let pem = p.clone().self_signed(&key).unwrap().pem();
        assert_eq!(not_after_der(&first_der(&pem).unwrap()).unwrap().to_rfc3339(), "2031-07-04T00:00:00+00:00");
        // After 2049, the date is a GeneralizedTime.
        p.not_after = rcgen::date_time_ymd(2051, 1, 2);
        let pem = p.self_signed(&key).unwrap().pem();
        assert_eq!(not_after_der(&first_der(&pem).unwrap()).unwrap().to_rfc3339(), "2051-01-02T00:00:00+00:00");
        assert!(first_der("not a certificate").is_none());
        assert!(not_after_der(&[0x30, 0x03, 0x02, 0x01]).is_none());
    }
}
