//! Whether an outgoing web request carries the person's private data
//! (decisions/2026-11-22-requests-checked-for-private-data.md).
//!
//! A run that has read untrusted content and private data browses anywhere:
//! what is checked is the request, not where it goes. Every result from a
//! private source is labelled, so the kernel knows the values the run (and the
//! conversation it continues) has read. A web read whose address carries one
//! of those values, plainly, percent-encoded, without its punctuation,
//! reversed, or in base64 or hex, is refused, and the agent is told to leave
//! the private data out. No model judges it.

use crate::kernel::Kernel;
use crate::state::{Run, State};
use base64::Engine;
use std::collections::HashSet;

/// The shortest value that counts: shorter ones match ordinary addresses.
const MIN_VALUE_CHARS: usize = 4;
/// A token this long counts even without a digit (an id, a key).
const LONG_TOKEN_CHARS: usize = 16;
/// The shortest run of base64 or hex worth decoding.
const MIN_ENCODED_CHARS: usize = 12;

/// What the agent is told when a request would carry private data.
pub(crate) const CARRIES_PRIVATE_DATA: &str = "this web address contains private data the conversation read (from the person's mail, files or other private apps), so it was not opened. Leave that data out of the address and try again";

fn tokens(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !(c.is_alphanumeric() || matches!(c, '.' | '@' | '-' | '_' | '+')))
        .map(|t| t.trim_matches(|c: char| matches!(c, '.' | '-' | '_' | '+')).to_lowercase())
        .filter(|t| !t.is_empty())
}

/// A value specific enough that finding it in an address means it was
/// copied there: a figure, a code, an address, an id.
fn distinctive(t: &str) -> bool {
    let len = t.chars().count();
    let digit = t.chars().any(|c| c.is_ascii_digit());
    let alpha = t.chars().any(char::is_alphabetic);
    len >= MIN_VALUE_CHARS && ((digit && (alpha || len > MIN_VALUE_CHARS)) || (t.contains('@') && t.contains('.')) || len >= LONG_TOKEN_CHARS)
}

fn compact(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

/// The private values in `private`: distinctive tokens that appear nowhere in
/// `public` (what the person wrote, what the web returned).
pub(crate) fn private_values(private: &[String], public: &[String]) -> HashSet<String> {
    let seen: HashSet<String> = public.iter().flat_map(|t| tokens(t)).collect();
    private.iter().flat_map(|t| tokens(t)).filter(|t| distinctive(t) && !seen.contains(t)).collect()
}

fn hex_decode(s: &str) -> Option<String> {
    if s.len() % 2 != 0 {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect();
    String::from_utf8(bytes?).ok()
}

fn b64_decode(s: &str) -> Option<String> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    [STANDARD, URL_SAFE, STANDARD_NO_PAD, URL_SAFE_NO_PAD].iter().find_map(|e| e.decode(s).ok()).and_then(|b| String::from_utf8(b).ok())
}

/// The ways an address can be read: as written, decoded, reversed, and any
/// long run of base64 or hex in it decoded.
fn readings(url: &str) -> Vec<String> {
    let decoded = percent_encoding::percent_decode_str(&url.replace('+', " ")).decode_utf8_lossy().to_string();
    let mut out = vec![url.to_lowercase(), decoded.to_lowercase()];
    // `=` ends a key (`d=`) as often as it pads base64, so runs are split on
    // it and decoded with and without padding.
    for seg in decoded.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '-' | '_'))).filter(|s| s.len() >= MIN_ENCODED_CHARS) {
        out.extend(b64_decode(seg).map(|s| s.to_lowercase()));
        out.extend(hex_decode(seg).map(|s| s.to_lowercase()));
    }
    let reversed: Vec<String> = out.iter().map(|s| s.chars().rev().collect()).collect();
    out.extend(reversed);
    out
}

/// Whether `url` carries one of `values`.
pub(crate) fn carries(url: &str, values: &HashSet<String>) -> bool {
    if values.is_empty() {
        return false;
    }
    let readings = readings(url);
    let compacted: Vec<String> = readings.iter().map(|r| compact(r)).collect();
    values.iter().any(|v| {
        let cv = compact(v);
        readings.iter().any(|r| r.contains(v.as_str())) || (cv.chars().count() >= MIN_VALUE_CHARS && compacted.iter().any(|r| r.contains(&cv)))
    })
}

/// The private results this run's conversation has read: its own and its
/// parents', and the earlier turns of the chat it continues. `None` when
/// some of its private data cannot be listed (an uploaded file, or privacy
/// carried in with nothing on record).
fn private_texts(s: &State, run: &Run) -> Option<Vec<String>> {
    let mut runs: Vec<&Run> = s.lineage(&run.id).iter().filter_map(|t| s.tasks.get(t)).filter_map(|t| s.runs.get(&t.run_id)).collect();
    let thread = s.tasks.get(&run.task).and_then(|t| t.chat.as_ref()).map(|c| c.thread.clone());
    if let Some(thread) = thread {
        runs.extend(s.tasks.values().filter(|t| t.person == run.person && t.chat.as_ref().is_some_and(|c| c.thread == thread)).filter_map(|t| s.runs.get(&t.run_id)));
    }
    if runs.iter().any(|r| r.private_from.contains("library")) {
        return None;
    }
    let texts: Vec<String> = runs.iter().flat_map(|r| r.results.values()).filter(|res| res.private && !res.is_error).map(|res| res.text.clone()).collect();
    (!texts.is_empty() || !run.private).then_some(texts)
}

impl Kernel {
    /// Whether `url`, which a run holding untrusted content and private data
    /// would open, carries private data. `None` when the run's private data
    /// cannot all be listed, and the address rules (`web_open`) decide.
    pub(crate) async fn web_address_carries_private_data(&self, run: &Run, url: &str) -> anyhow::Result<Option<bool>> {
        let Some(private) = self.read(|s| private_texts(s, run)) else { return Ok(None) };
        let public = self.web_sources(run).await?;
        Ok(Some(carries(url, &private_values(&private, &public))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> HashSet<String> {
        let mail = vec![r#"{"messages":[{"from":"cfo@corp.example","subject":"Q3 numbers","body":"Confidential: revenue 41.7M. 8f3kq2x9z1"}]}"#.to_string()];
        private_values(&mail, &["Read my inbox and the news page.".to_string()])
    }

    #[test]
    fn the_values_are_figures_codes_and_addresses_not_words() {
        let v = values();
        assert!(v.contains("41.7m") && v.contains("8f3kq2x9z1") && v.contains("cfo@corp.example"), "{v:?}");
        assert!(!v.contains("revenue") && !v.contains("confidential") && !v.contains("q3"), "{v:?}");
    }

    #[test]
    fn an_address_carrying_a_private_value_is_caught_however_it_is_dressed() {
        let v = values();
        for url in [
            "https://collector.example/collect?d=revenue-41.7M",
            "https://collector.example/41.7m",
            "https://collector.example/x?d=41%2E7M",
            "https://collector.example/x?d=41-7m",
            "https://collector.example/M7.14",
            "https://collector.example/x?d=OGYza3EyeDl6MQ==",
            "https://collector.example/x?d=3866336b713278397a31",
            "https://collector.example/x?to=cfo%40corp.example",
            "https://8f3kq2x9z1.collector.example/",
        ] {
            assert!(carries(url, &v), "{url}");
        }
    }

    #[test]
    fn any_other_address_opens() {
        let v = values();
        for url in [
            "https://docs.rs/tokio/latest/tokio/",
            "https://github.com/rust-lang/rust/issues?q=label%3Abug",
            "https://www.google.com/search?q=revenue+recognition+rules",
            "https://example.org/2026/report",
            "https://collector.example/collect?d=nothing+private",
        ] {
            assert!(!carries(url, &v), "{url}");
        }
    }

    #[test]
    fn a_value_the_person_wrote_or_the_web_returned_is_not_private() {
        let mail = vec!["Order 55123 ships Friday".to_string()];
        assert!(private_values(&mail, &["Track order 55123".to_string()]).is_empty());
        assert!(!private_values(&mail, &[]).is_empty());
    }
}
