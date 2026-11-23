//! One-time codes, password reset links and sign-in links are taken out of
//! mail before it reaches the agent. They are live credentials, and an agent
//! that can read one can be told to use one
//! (`decisions/2026-11-23-login-codes-out-of-all-mail.md`).
//!
//! A line that speaks of a code, and the next line with text on it, lose
//! every word that reads as one: digits, digits in equal groups ("123 456",
//! "123-456"), and longer runs of letters and digits. A link leading into an
//! account goes: one that names it (reset, login, token...), and, on a line
//! that speaks of signing in or the line after, one carrying a long token.

use crate::config::Connector;

const CODE_REMOVED: &str = "[one-time code removed]";
const LINK_REMOVED: &str = "[sign-in link removed]";
/// Words that make a code-like word on their line, or on the next line with
/// text, a login code. Each is a whole word (or its plural).
const CODE_WORDS: [&str; 13] =
    ["code", "otp", "one-time", "one time", "passcode", "pin", "2fa", "two-factor", "verification", "verify", "authenticate", "authentication", "security"];
/// Words that make a link carrying a token, on their line or the next line
/// with text, a sign-in link. Each begins a word.
const SIGN_IN_WORDS: [&str; 12] = ["sign in", "sign-in", "signin", "log in", "log-in", "login", "magic", "reset", "confirm", "activate", "unlock", "password"];
/// A link holding one of these is a way into an account.
const LINK_WORDS: [&str; 16] =
    ["reset", "password", "passwd", "signin", "sign-in", "sign_in", "login", "log-in", "magic", "verify", "confirm", "activate", "auth", "token", "otp", "unlock"];
/// Words in an app's name, title or address, or a tool's name, that make
/// it a mail app.
const MAIL_WORDS: [&str; 4] = ["mail", "inbox", "imap", "outlook"];
const ESCAPED_NEWLINE: &str = "\\n";
const MIN_CODE_CHARS: usize = 4;
/// Up to this long, a code is at least half digits; longer, it mixes
/// letters with at least `MIN_LONG_CODE_DIGITS` digits.
const MAX_SHORT_CODE_CHARS: usize = 10;
const MAX_CODE_CHARS: usize = 64;
const MIN_LONG_CODE_DIGITS: usize = 2;
/// Digits in one group of a grouped code, fewest and most.
const MIN_GROUP_DIGITS: usize = 2;
const MAX_GROUP_DIGITS: usize = 4;
/// A run of letters, digits, `-` and `_` this long, with a digit, in a
/// link's path or query: the token a sign-in link carries.
const MIN_LINK_TOKEN_CHARS: usize = 16;

/// Whether what `tool` of app `c` returns has its codes and sign-in links
/// taken out: a mail app's, or any app's that asks for it, unless the app
/// keeps them (`keep_login_codes`, the opt-out). A mail app is one in the
/// catalog's email category (which sets `filter_login_codes`), or one whose
/// name, title or address, or the tool's own name, names mail.
pub fn applies(c: &Connector, tool: &str) -> bool {
    if c.keep_login_codes {
        return false;
    }
    let host = url::Url::parse(&c.url).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase)).unwrap_or_default();
    let own = tool.split_once("__").map_or(tool, |(_, t)| t);
    let names = [c.name.to_ascii_lowercase(), c.title.clone().unwrap_or_default().to_ascii_lowercase(), host, own.to_ascii_lowercase()];
    c.filter_login_codes || names.iter().any(|n| MAIL_WORDS.iter().any(|w| n.contains(w)))
}

impl Connector {
    /// `applies`: whether `tool`'s results lose their codes and sign-in links.
    pub fn filters_login_codes(&self, tool: &str) -> bool {
        applies(self, tool)
    }
}

/// Whether `lower` holds `word` starting a word, and, when `whole`, ending
/// one too (a plural `s` allowed).
fn mentions(lower: &str, word: &str, whole: bool) -> bool {
    let b = lower.as_bytes();
    lower.match_indices(word).any(|(i, _)| {
        let starts = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        let end = i + word.len();
        let after = |j: usize| j >= b.len() || !b[j].is_ascii_alphanumeric();
        starts && (!whole || after(end) || (b[end] == b's' && after(end + 1)))
    })
}

fn speaks_of_code(lower: &str) -> bool {
    CODE_WORDS.iter().any(|w| mentions(lower, w, true))
}

fn speaks_of_sign_in(lower: &str) -> bool {
    speaks_of_code(lower) || SIGN_IN_WORDS.iter().any(|w| mentions(lower, w, false))
}

fn looks_like_code(word: &str) -> bool {
    let (n, digits) = (word.len(), word.chars().filter(|c| c.is_ascii_digit()).count());
    if !(MIN_CODE_CHARS..=MAX_CODE_CHARS).contains(&n) || !word.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    if n <= MAX_SHORT_CODE_CHARS {
        digits >= n / 2
    } else {
        digits >= MIN_LONG_CODE_DIGITS
    }
}

fn is_url_char(c: char) -> bool {
    !c.is_whitespace() && !matches!(c, '<' | '>' | '"' | '\'' | ')' | ']' | '\\')
}

/// Whether a link's path or query carries a long token.
fn carries_token(url: &str) -> bool {
    let after_scheme = url.split_once("://").map_or(url, |(_, r)| r);
    let Some((_, rest)) = after_scheme.split_once(['/', '?']) else { return false };
    rest.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).any(|run| run.len() >= MIN_LINK_TOKEN_CHARS && run.chars().any(|c| c.is_ascii_digit()))
}

/// Replaces every link in the line that leads into an account; with
/// `sign_in`, every link carrying a token too.
fn without_login_links(line: &str, sign_in: bool) -> String {
    let lower = line.to_ascii_lowercase();
    let mut out = String::new();
    let mut at = 0;
    while let Some(found) = lower[at..].find("http").map(|i| i + at) {
        let rest = &lower[found..];
        if !(rest.starts_with("http://") || rest.starts_with("https://")) {
            out.push_str(&line[at..found + 4]);
            at = found + 4;
            continue;
        }
        let end = found + rest.find(|c: char| !is_url_char(c)).unwrap_or(rest.len());
        out.push_str(&line[at..found]);
        let url = &lower[found..end];
        if LINK_WORDS.iter().any(|w| url.contains(w)) || (sign_in && carries_token(&line[found..end])) {
            out.push_str(LINK_REMOVED);
        } else {
            out.push_str(&line[found..end]);
        }
        at = end;
    }
    out.push_str(&line[at..]);
    out
}

/// Replaces digits in equal groups of two to four, joined by single spaces
/// or hyphens ("123 456", "123-456", "12 34 56"): one code. Unequal groups
/// (a phone number, a date) are left to the word rule.
fn without_grouped_codes(line: &str) -> String {
    let b = line.as_bytes();
    let (mut out, mut copied, mut i) = (String::new(), 0, 0);
    while i < b.len() {
        if !b[i].is_ascii_digit() || (i > 0 && b[i - 1].is_ascii_alphanumeric()) {
            i += 1;
            continue;
        }
        let (start, mut j, mut groups) = (i, i, vec![]);
        loop {
            let g = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            groups.push(j - g);
            if j + 1 < b.len() && matches!(b[j], b' ' | b'-') && b[j + 1].is_ascii_digit() {
                j += 1;
            } else {
                break;
            }
        }
        let ends = j == b.len() || !b[j].is_ascii_alphanumeric();
        let equal = groups.len() >= 2 && (MIN_GROUP_DIGITS..=MAX_GROUP_DIGITS).contains(&groups[0]) && groups.iter().all(|n| *n == groups[0]);
        if ends && equal {
            out.push_str(&line[copied..start]);
            out.push_str(CODE_REMOVED);
            copied = j;
        }
        i = j;
    }
    out.push_str(&line[copied..]);
    out
}

/// Replaces every word that reads as a code. An email address is never a
/// code, whatever its digits.
fn without_codes(line: &str) -> String {
    let line = without_grouped_codes(line);
    let mut out = String::new();
    for (i, token) in line.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        if token.contains('@') && token.contains('.') {
            out.push_str(token);
        } else {
            out.push_str(&without_codes_in(token));
        }
    }
    out
}

fn without_codes_in(text: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if looks_like_code(word) {
            out.push_str(CODE_REMOVED);
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// What the line before said: of a code, of signing in.
#[derive(Clone, Copy, Default)]
struct Window {
    code: bool,
    sign_in: bool,
}

/// One line, cleaned, and the window it opens for the next line with text.
fn cleaned(line: &str, before: Window) -> (String, Window) {
    let lower = line.to_ascii_lowercase();
    let here = Window { code: speaks_of_code(&lower), sign_in: speaks_of_sign_in(&lower) };
    let linked = without_login_links(line, here.sign_in || before.sign_in);
    let out = if here.code || before.code { without_codes(&linked) } else { linked };
    // A blank line keeps the window open: "Your code:", a gap, then the code.
    (out, if line.trim().is_empty() { before } else { here })
}

/// Text as the agent may read it. A connector's JSON result is one line, so
/// each of its strings is read on its own: a code word in a mail's body
/// must not make the address or date beside it look like a code.
pub fn strip(text: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(mut v) if v.is_object() || v.is_array() => {
            strip_value(&mut v);
            v.to_string()
        }
        _ => strip_text(text),
    }
}

fn strip_value(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::String(s) => *s = strip(s),
        serde_json::Value::Array(a) => a.iter_mut().for_each(strip_value),
        serde_json::Value::Object(o) => o.values_mut().for_each(strip_value),
        _ => {}
    }
}

/// Line by line, where a line break is a newline or, in a connector's JSON
/// result, a written `\n`.
fn strip_text(text: &str) -> String {
    let mut window = Window::default();
    let mut lines = vec![];
    for line in text.split('\n') {
        let mut parts = vec![];
        for part in line.split(ESCAPED_NEWLINE) {
            let (out, next) = cleaned(part, window);
            window = next;
            parts.push(out);
        }
        lines.push(parts.join(ESCAPED_NEWLINE));
    }
    lines.join("\n")
}

#[cfg(test)]
#[path = "login_codes_tests.rs"]
mod tests;
