//! A computer holds a surrogate; the real credential is only ever added here,
//! on its way to the one host it is for, after the approval authority allows
//! that request. The computer never receives it.
use super::http::Head;
use crate::kernel::Kernel;
use sha2::{Digest, Sha256};

pub(in crate::computer) const PREFIX: &str = "msur_";
/// Hex of a 256-bit token.
const TOKEN_CHARS: usize = 64;

pub(in crate::computer) fn hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub(in crate::computer) fn vault_name(computer: &str, name: &str) -> String {
    format!("surrogate/{computer}/{name}")
}

/// Every surrogate-shaped token in a header's value.
fn tokens(value: &str) -> Vec<&str> {
    value.match_indices(PREFIX).filter_map(|(at, _)| value.get(at..at + PREFIX.len() + TOKEN_CHARS)).filter(|t| t[PREFIX.len()..].bytes().all(|b| b.is_ascii_hexdigit())).collect()
}

fn bound_to(pattern: &str, host: &str) -> bool {
    match pattern.strip_prefix("*.") {
        Some(suffix) => host.len() > suffix.len() + 1 && host.ends_with(suffix) && host.as_bytes()[host.len() - suffix.len() - 1] == b'.',
        None => pattern.eq_ignore_ascii_case(host),
    }
}

/// The head to send: with real credentials if every surrogate in it may be
/// swapped here, or the reason not to send it at all. Nothing partial goes.
pub(super) async fn swapped(k: &Kernel, computer: &str, mut head: Head, host: &str) -> Result<(Head, Scrubber), (Head, String)> {
    let Some(c) = k.read(|s| s.computers.get(computer).cloned()) else { return Ok((head, Scrubber::default())) };
    let mut swaps: Vec<(usize, String, String)> = vec![];
    for (index, (_, value)) in head.headers.iter().enumerate() {
        for token in tokens(value) {
            let Some(found) = c.surrogates.iter().find(|s| s.hash == hash(token)) else { continue };
            if !bound_to(&found.host, host) {
                let reason = format!("the credential {} is not for {host}", found.name);
                return Err((head, reason));
            }
            match k.vault.get(&vault_name(computer, &found.name)) {
                Ok(Some(secret)) => swaps.push((index, token.to_string(), secret.expose().to_string())),
                _ => {
                    let reason = format!("the credential {} is not in the vault", found.name);
                    return Err((head, reason));
                }
            }
        }
    }
    if swaps.is_empty() {
        return Ok((head, Scrubber::default()));
    }
    let person = match &c.owner {
        crate::computer::Owner::Person(p) => p.clone(),
        crate::computer::Owner::Workspace(w) => format!("workspace:{w}"),
    };
    use crate::auto_review::Verdict;
    match k.review_credential_use(&person, computer, &head.method, host, &head.target).await {
        Verdict::Allow(_) => {}
        Verdict::Deny(reason) | Verdict::Ask(reason) => return Err((head, format!("the approval authority did not allow this credential: {reason}"))),
    }
    let mut scrubber = Scrubber::default();
    for (index, token, real) in swaps {
        let value = head.headers[index].1.replace(&token, &real);
        head.headers[index].1 = value;
        scrubber.add(real, token);
    }
    Ok((head, scrubber))
}

/// Puts the surrogate back wherever the destination repeats the real value.
/// A stream can split a value across two reads, so the end of each is held
/// back until the next.
#[derive(Default)]
pub(super) struct Scrubber {
    pairs: Vec<(Vec<u8>, Vec<u8>)>,
    held: Vec<u8>,
}

impl Scrubber {
    fn add(&mut self, real: String, token: String) {
        if real.len() >= MIN_ECHO_BYTES {
            self.pairs.push((real.into_bytes(), token.into_bytes()));
        }
    }

    pub(super) fn active(&self) -> bool {
        !self.pairs.is_empty()
    }

    fn replaced(&self, mut bytes: Vec<u8>) -> Vec<u8> {
        for (real, token) in &self.pairs {
            let mut out = Vec::with_capacity(bytes.len());
            let mut at = 0;
            while at < bytes.len() {
                if bytes[at..].starts_with(real) {
                    out.extend_from_slice(token);
                    at += real.len();
                } else {
                    out.push(bytes[at]);
                    at += 1;
                }
            }
            bytes = out;
        }
        bytes
    }

    pub(super) fn text(&self, line: &str) -> String {
        String::from_utf8_lossy(&self.replaced(line.as_bytes().to_vec())).into_owned()
    }

    /// What can be sent now.
    pub(super) fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        let longest = self.pairs.iter().map(|(r, _)| r.len()).max().unwrap_or(0);
        if longest == 0 {
            return chunk.to_vec();
        }
        let mut all = std::mem::take(&mut self.held);
        all.extend_from_slice(chunk);
        let keep = (longest - 1).min(all.len());
        let cut = all.len() - keep;
        // A value starting before the cut and ending after it must be whole.
        let mut head = all[..cut].to_vec();
        let tail = all[cut..].to_vec();
        let spans = self.pairs.iter().any(|(real, _)| (1..real.len()).any(|n| head.ends_with(&real[..n]) && tail.starts_with(&real[n..])));
        if spans {
            self.held = all;
            head = vec![];
        } else {
            self.held = tail;
        }
        self.replaced(head)
    }

    /// What was held back, at the end of the stream.
    pub(super) fn finish(&mut self) -> Vec<u8> {
        let held = std::mem::take(&mut self.held);
        self.replaced(held)
    }
}

/// Anything shorter would turn ordinary text into surrogates.
const MIN_ECHO_BYTES: usize = 8;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repeated_secret_goes_back_to_its_surrogate_however_it_is_split() {
        let body = b"before REAL-SECRET-VALUE middle REAL-SECRET-VALUE end";
        for size in 1..body.len() {
            let mut s = Scrubber::default();
            s.add("REAL-SECRET-VALUE".into(), "surrogate".into());
            let mut out = vec![];
            for chunk in body.chunks(size) {
                out.extend(s.push(chunk));
            }
            out.extend(s.finish());
            assert_eq!(String::from_utf8(out).unwrap(), "before surrogate middle surrogate end", "chunks of {size}");
        }
    }

    #[test]
    fn with_nothing_swapped_the_stream_is_untouched_and_a_short_value_is_ignored() {
        let mut none = Scrubber::default();
        assert_eq!(none.push(b"abc"), b"abc");
        assert!(!none.active());
        let mut short = Scrubber::default();
        short.add("abc".into(), "zzz".into());
        assert!(!short.active());
    }

    #[test]
    fn only_a_whole_token_counts() {
        let t = format!("{PREFIX}{}", "a".repeat(TOKEN_CHARS));
        assert_eq!(tokens(&format!("Bearer {t}")), vec![t.as_str()]);
        assert!(tokens(&format!("{PREFIX}short")).is_empty());
        assert!(tokens(&format!("{PREFIX}{}", "z".repeat(TOKEN_CHARS))).is_empty());
    }

    #[test]
    fn a_surrogate_is_bound_to_its_host() {
        assert!(bound_to("api.example.com", "API.example.com"));
        assert!(!bound_to("api.example.com", "api.example.com.evil.test"));
        assert!(bound_to("*.example.com", "a.example.com"));
        assert!(!bound_to("*.example.com", "example.com"));
    }
}
