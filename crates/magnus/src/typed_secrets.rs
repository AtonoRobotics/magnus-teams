//! Secrets typed into chat. A password, key, token or card number a person
//! types into a message is outside the vault's protection, so the kernel
//! masks it before the message is written to the log: no stored copy, no
//! recalled conversation and no model call ever holds the clear value
//! (spec, Credentials and vault: "Typed secrets"). What stays in its place
//! is a marker such as `[hidden password]`, which tells the model and the
//! person what was there without what it was. The client steers the person
//! to the secure form instead.
//!
//! Detection is deliberately narrow: well-known key and token formats,
//! private key blocks, card numbers that pass the Luhn check with a card
//! network's prefix, credentials in a URL, and a value given right after a
//! label such as "password:" or "my API key is". Ordinary words, hashes and
//! ids are left alone.

use crate::events::Ev;
use regex::Regex;
use std::sync::LazyLock;

/// What a masked value is replaced with starts with this.
pub const MARKER_PREFIX: &str = "[hidden ";

/// The fewest characters a labelled value must have to be masked.
const MIN_LABELLED_VALUE_CHARS: usize = 4;

/// The digits a card number can have.
const CARD_DIGITS: std::ops::RangeInclusive<usize> = 13..=19;

/// A message with what was hidden in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Masked {
    pub text: String,
    /// What each hidden value was, in order ("password", "API key"...).
    pub hidden: Vec<&'static str>,
}

/// Each well-known format, with what it is called once hidden. The whole
/// match is hidden.
static FORMATS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        (r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----(?s:.*?)(?:-----END [A-Z0-9 ]*PRIVATE KEY-----|\z)", "private key"),
        (r"\bsk-(?:ant-|proj-)?[A-Za-z0-9_\-]{20,}", "API key"),
        (r"\b[sr]k_(?:live|test)_[A-Za-z0-9]{16,}", "API key"),
        (r"\bgh[pousr]_[A-Za-z0-9]{30,}", "access token"),
        (r"\bgithub_pat_[A-Za-z0-9_]{30,}", "access token"),
        (r"\bglpat-[A-Za-z0-9_\-]{20,}", "access token"),
        (r"\bxox[abposr]-[A-Za-z0-9\-]{10,}", "access token"),
        (r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", "API key"),
        (r"\bAIza[0-9A-Za-z_\-]{35}", "API key"),
        (r"\bhf_[A-Za-z0-9]{30,}", "access token"),
        (r"\bnpm_[A-Za-z0-9]{36}", "access token"),
        (r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}", "access token"),
    ]
    .into_iter()
    .filter_map(|(re, what)| Regex::new(re).ok().map(|r| (r, what)))
    .collect()
});

/// Formats where only one group is the secret.
static PARTS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        // The password in scheme://user:password@host.
        (r"(?i)\b[a-z][a-z0-9+.\-]*://[^\s:/@]+:([^\s@/]+)@", "password"),
        (r"(?i)\bbearer\s+([A-Za-z0-9._~+/=\-]{20,})", "access token"),
    ]
    .into_iter()
    .filter_map(|(re, what)| Regex::new(re).ok().map(|r| (r, what)))
    .collect()
});

/// A label, up to two "for X" words, a separator, and the value.
static LABELLED: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b(passwords?|passwd|pwd|passcode|passphrase|pin|api[ _\-]?key|client[ _\-]?secret|secret(?:[ _\-]?key)?|access[ _\-]?token|auth[ _\-]?token|token)\b((?:\s+(?:for|to|of|on|at)\s+[^\s:=]+){0,2})\s*(:|=|\bis\b|\bwas\b)\s*["'`]?([^\s"'`]+)"#,
    )
    .ok()
});

/// Card numbers: digits, with single spaces or dashes between groups.
static CARD: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\b\d(?:[ \-]?\d){12,18}\b").ok());

/// Markers already in the text, so masking twice changes nothing.
static MARKER: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\[hidden [A-Za-z ]+?(?: ending \d{4})?\]").ok());

/// Hide every secret in `text`.
pub fn mask(text: &str) -> Masked {
    let kept: Vec<(usize, usize)> = MARKER.iter().flat_map(|m| m.find_iter(text).map(|x| (x.start(), x.end()))).collect();
    let mut found: Vec<(usize, usize, String, &'static str)> = vec![];
    for (re, what) in FORMATS.iter() {
        for m in re.find_iter(text) {
            found.push((m.start(), m.end(), format!("[hidden {what}]"), what));
        }
    }
    for (re, what) in PARTS.iter() {
        for c in re.captures_iter(text) {
            if let Some(g) = c.get(1) {
                found.push((g.start(), g.end(), format!("[hidden {what}]"), what));
            }
        }
    }
    for c in LABELLED.iter().flat_map(|re| re.captures_iter(text)) {
        let (Some(label), Some(sep), Some(value)) = (c.get(1), c.get(3), c.get(4)) else { continue };
        let what = labelled_kind(label.as_str());
        if labelled_secret(what, sep.as_str(), value.as_str()) {
            found.push((value.start(), value.end(), format!("[hidden {what}]"), what));
        }
    }
    for m in CARD.iter().flat_map(|re| re.find_iter(text)) {
        let digits: String = m.as_str().chars().filter(char::is_ascii_digit).collect();
        if is_card(&digits) {
            let last = &digits[digits.len() - 4..];
            found.push((m.start(), m.end(), format!("[hidden card number ending {last}]"), "card number"));
        }
    }
    // Earliest first, the longest of those that start together; anything
    // overlapping one already taken, or a marker, is left out.
    found.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut out = String::with_capacity(text.len());
    let mut hidden = vec![];
    let mut at = 0;
    for (start, end, marker, what) in found {
        if start < at || kept.iter().any(|&(s, e)| start < e && s < end) {
            continue;
        }
        out.push_str(&text[at..start]);
        out.push_str(&marker);
        hidden.push(what);
        at = end;
    }
    out.push_str(&text[at..]);
    Masked { text: out, hidden }
}

fn labelled_kind(label: &str) -> &'static str {
    let l = label.to_ascii_lowercase();
    if l == "pin" {
        "PIN"
    } else if l.starts_with("pass") || l == "pwd" {
        "password"
    } else if l.starts_with("api") {
        "API key"
    } else if l.contains("token") {
        "access token"
    } else {
        "secret"
    }
}

/// Whether a value given after a label is the secret itself. After ":" or
/// "=" it is; after "is" or "was" only when it does not read as a word
/// ("the password is stored in the vault").
fn labelled_secret(what: &str, sep: &str, value: &str) -> bool {
    if value.starts_with(MARKER_PREFIX.trim_end()) {
        return false;
    }
    if what == "PIN" {
        return (4..=8).contains(&value.len()) && value.chars().all(|c| c.is_ascii_digit());
    }
    if value.chars().count() < MIN_LABELLED_VALUE_CHARS {
        return false;
    }
    if sep == ":" || sep == "=" {
        return true;
    }
    let core = value.trim_end_matches(['.', ',', ';', '!', '?', ')']);
    let digit = core.chars().any(|c| c.is_ascii_digit());
    let symbol = core.chars().any(|c| !c.is_alphanumeric());
    let inner_upper = core.chars().skip(1).any(char::is_uppercase) && core.chars().any(char::is_lowercase);
    digit || symbol || inner_upper
}

/// A card number: a card network's prefix, its length, and the Luhn check.
fn is_card(digits: &str) -> bool {
    if !CARD_DIGITS.contains(&digits.len()) {
        return false;
    }
    let two: u32 = digits[..2].parse().unwrap_or(0);
    let network = digits.starts_with('4')
        || (51..=55).contains(&two)
        || (22..=27).contains(&two)
        || two == 34
        || two == 37
        || two == 35
        || two == 36
        || two == 38
        || two == 30
        || digits.starts_with('6');
    if !network {
        return false;
    }
    let sum: u32 = digits
        .bytes()
        .rev()
        .enumerate()
        .map(|(i, b)| {
            let d = u32::from(b - b'0');
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 {
                    x - 9
                } else {
                    x
                }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// Mask what a person typed in an event about to be written: a task or
/// chat message, a schedule's task, a workspace channel message. Returns
/// what was hidden.
pub fn mask_event(ev: &mut Ev) -> Vec<&'static str> {
    let text = match ev {
        Ev::TaskCreated { text, .. } | Ev::ScheduleSet { text, .. } => text,
        Ev::Team(crate::team::Change::Message { message, .. }) => &mut message.text,
        _ => return vec![],
    };
    let m = mask(text);
    if !m.hidden.is_empty() {
        *text = m.text;
    }
    m.hidden
}

#[cfg(test)]
mod tests {
    use super::*;

    fn masked(t: &str) -> String {
        mask(t).text
    }

    #[test]
    fn labelled_passwords_are_hidden() {
        assert_eq!(masked("my password is hunter22 so log in for me"), "my password is [hidden password] so log in for me");
        assert_eq!(masked("Password: correcthorse"), "Password: [hidden password]");
        assert_eq!(masked("the password for github.com is Tr0ub4dor&3"), "the password for github.com is [hidden password]");
        assert_eq!(masked("pin=4821"), "pin=[hidden PIN]");
        assert_eq!(masked("api key: abcd1234efgh"), "api key: [hidden API key]");
    }

    #[test]
    fn known_formats_are_hidden() {
        let m = mask("use sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWx and ghp_abcdefghijklmnopqrstuvwxyz0123456789");
        assert_eq!(m.text, "use [hidden API key] and [hidden access token]");
        assert_eq!(m.hidden, vec!["API key", "access token"]);
        assert_eq!(masked("AKIAIOSFODNN7EXAMPLE"), "[hidden API key]");
        assert_eq!(masked("Authorization: Bearer abcdefghijklmnopqrstuvwxyz123"), "Authorization: Bearer [hidden access token]");
        assert_eq!(masked("postgres://sam:s3cret!@db.example.com/x"), "postgres://sam:[hidden password]@db.example.com/x");
        assert_eq!(masked("-----BEGIN RSA PRIVATE KEY-----\nMIIabc\n-----END RSA PRIVATE KEY-----"), "[hidden private key]");
    }

    #[test]
    fn card_numbers_keep_only_their_last_four() {
        assert_eq!(masked("pay with 4242 4242 4242 4242 please"), "pay with [hidden card number ending 4242] please");
        assert_eq!(masked("card 5555-5555-5555-4444"), "card [hidden card number ending 4444]");
        // Fails the Luhn check, or has no card network's prefix.
        assert_eq!(masked("order 4242 4242 4242 4241"), "order 4242 4242 4242 4241");
        assert_eq!(masked("at 1731196800000 ms"), "at 1731196800000 ms");
    }

    #[test]
    fn ordinary_text_is_left_alone() {
        for t in [
            "the password is stored in the vault",
            "commit b9eba28f3c1d2e4f5a6b7c8d9e0f1a2b3c4d5e6f",
            "call me at +1 415 555 0100",
            "the token was Expired.",
            "a pin is a short code",
            "https://example.com/a:b@c",
        ] {
            assert_eq!(masked(t), t, "{t}");
        }
    }

    #[test]
    fn masking_twice_changes_nothing() {
        let once = masked("password: hunter22 and 4242424242424242");
        assert_eq!(masked(&once), once);
    }

    #[test]
    fn a_chat_message_is_masked_before_it_is_written() {
        let mut ev: Ev = serde_json::from_value(serde_json::json!({
            "kind": "task_created", "task": "t", "run": "r", "person": "sam",
            "text": "sign in to acme with password: hunter22",
            "config": { "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "verifier": false, "instructions": "", "harness": "",
                        "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 } },
        }))
        .unwrap();
        assert_eq!(mask_event(&mut ev), vec!["password"]);
        let Ev::TaskCreated { text, .. } = &ev else { unreachable!() };
        assert_eq!(text, "sign in to acme with password: [hidden password]");
        let mut said = Ev::Team(crate::team::Change::Message {
            workspace: "w".into(),
            channel: "c".into(),
            message: crate::team::Message { text: "card 4242424242424242".into(), ..Default::default() },
        });
        assert_eq!(mask_event(&mut said), vec!["card number"]);
        let Ev::Team(crate::team::Change::Message { message, .. }) = &said else { unreachable!() };
        assert_eq!(message.text, "card [hidden card number ending 4242]");
    }
}
