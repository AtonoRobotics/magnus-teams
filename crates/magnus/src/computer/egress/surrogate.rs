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
pub(super) async fn swapped(k: &Kernel, computer: &str, mut head: Head, host: &str) -> Result<Head, (Head, String)> {
    let Some(c) = k.read(|s| s.computers.get(computer).cloned()) else { return Ok(head) };
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
        return Ok(head);
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
    for (index, token, real) in swaps {
        let value = head.headers[index].1.replace(&token, &real);
        head.headers[index].1 = value;
    }
    Ok(head)
}

#[cfg(test)]
mod tests {
    use super::*;

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
