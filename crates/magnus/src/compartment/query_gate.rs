//! Shared query guardrails for `web_search` and `research_web`.
//!
//! When a run holds both private data and untrusted content, every outbound
//! search or research query passes through this gate:
//! 1. Deterministic secret/PII scrubbing (always on, even on clean runs).
//! 2. A per-run byte budget (`MAX_TAINTED_QUERY_BYTES_PER_RUN`) shared across
//!    `web_search` and `research_web`.
//! 3. A mandatory outgoing classifier check (`safety::outgoing_finding`) when
//!    both `private` and `untrusted` are present in the run's context.
//! 4. A denial retry cap (`MAX_QUERY_DENIALS_PER_RUN`) so an injected model
//!    cannot iteratively probe the classifier.
use crate::boundary::Action;
use crate::kernel::Kernel;
use crate::state::Run;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Mutex;

/// Maximum total UTF-8 bytes allowed across all `web_search` and `research_web`
/// queries in a single tainted (`private && untrusted`) run.
pub const MAX_TAINTED_QUERY_BYTES_PER_RUN: usize = 2048;

/// Maximum number of query denials in a single tainted run before further
/// queries are hard-denied (prevents classifier oracle probing).
pub const MAX_QUERY_DENIALS_PER_RUN: u32 = 3;

/// Maximum runs tracked in the in-memory query budget table before pruning.
const MAX_TRACKED_QUERY_RUNS: usize = 4096;

/// Minimum token length treated as a potential opaque secret/bearer token.
const MIN_OPAQUE_SECRET_LEN: usize = 32;

/// Well-known secret/API key prefixes that must never leave in a web query.
const SECRET_PREFIXES: [&str; 9] = ["sk-", "ghp_", "gho_", "github_pat_", "xoxb-", "xoxp-", "AIza", "AKIA", "-----BEGIN "];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueryBudget {
    pub bytes_used: usize,
    pub denials: u32,
}

static BUDGETS: Mutex<Option<HashMap<String, QueryBudget>>> = Mutex::new(None);

fn with_budget<R>(run_key: &str, f: impl FnOnce(&mut QueryBudget) -> R) -> R {
    let mut guard = BUDGETS.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    if map.len() >= MAX_TRACKED_QUERY_RUNS && !map.contains_key(run_key) {
        map.clear();
    }
    f(map.entry(run_key.to_string()).or_default())
}

#[cfg(test)]
pub(crate) fn reset_budget_for_test(run_key: &str) {
    let mut guard = BUDGETS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(map) = guard.as_mut() {
        map.remove(run_key);
    }
}

/// Returns `Some(reason)` if `query` contains a deterministic secret or PII
/// pattern (API key prefix, SSN, Luhn-valid card number, or opaque token).
pub fn scrub_check(query: &str) -> Option<String> {
    for prefix in SECRET_PREFIXES {
        if query.contains(prefix) {
            return Some(format!("query contains a credential-like prefix ({prefix})"));
        }
    }
    if contains_ssn(query) {
        return Some("query contains a Social Security Number pattern".into());
    }
    if contains_card_number(query) {
        return Some("query contains a payment card number".into());
    }
    if contains_opaque_secret_token(query) {
        return Some("query contains a high-entropy opaque token".into());
    }
    None
}

/// Checks for `NNN-NN-NNNN` surrounded by non-digits.
fn contains_ssn(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 11 {
        return false;
    }
    for i in 0..=b.len() - 11 {
        let before_ok = i == 0 || !b[i - 1].is_ascii_digit();
        let after_ok = i + 11 == b.len() || !b[i + 11].is_ascii_digit();
        if !before_ok || !after_ok {
            continue;
        }
        let w = &b[i..i + 11];
        if w[3] == b'-' && w[6] == b'-' && w[0..3].iter().all(u8::is_ascii_digit) && w[4..6].iter().all(u8::is_ascii_digit) && w[7..11].iter().all(u8::is_ascii_digit) {
            return true;
        }
    }
    false
}

/// Checks for a 13..=19-digit Luhn-valid sequence (allowing spaces or hyphens).
fn contains_card_number(s: &str) -> bool {
    let mut digits = Vec::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            digits.push((ch as u8) - b'0');
        } else if ch == ' ' || ch == '-' {
            continue;
        } else {
            if (13..=19).contains(&digits.len()) && luhn_valid(&digits) {
                return true;
            }
            digits.clear();
        }
    }
    (13..=19).contains(&digits.len()) && luhn_valid(&digits)
}

fn luhn_valid(digits: &[u8]) -> bool {
    let mut sum: u32 = 0;
    for (idx, &d) in digits.iter().rev().enumerate() {
        let mut v = d as u32;
        if idx % 2 == 1 {
            v *= 2;
            if v > 9 {
                v -= 9;
            }
        }
        sum += v;
    }
    sum % 10 == 0
}

/// Detects long (`>= 32`) mixed alphanumeric/base64 tokens, including those
/// embedded in URL path or query segments.
fn is_opaque_token_segment(seg: &str) -> bool {
    if seg.len() < MIN_OPAQUE_SECRET_LEN {
        return false;
    }
    let valid_chars = seg.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'+' | b'/' | b'='));
    if !valid_chars {
        return false;
    }
    let has_digit = seg.bytes().any(|b| b.is_ascii_digit());
    let has_alpha = seg.bytes().any(|b| b.is_ascii_alphabetic());
    has_digit && has_alpha
}

fn is_opaque_url_segment(seg: &str) -> bool {
    if !is_opaque_token_segment(seg) {
        return false;
    }
    // Normal blog/doc URL slugs use multiple lowercase hyphen-separated words;
    // opaque secrets in URLs either have <= 1 hyphen or mix upper/lower/digits.
    let hyphens = seg.bytes().filter(|&b| b == b'-').count();
    let has_upper = seg.bytes().any(|b| b.is_ascii_uppercase());
    hyphens <= 1 || has_upper
}

fn contains_opaque_secret_token(s: &str) -> bool {
    for tok in s.split_whitespace() {
        let cleaned = tok.trim_matches(|c: char| matches!(c, '"' | '\'' | '(' | ')' | '[' | ']' | ',' | ';' | '.'));
        if cleaned.len() < MIN_OPAQUE_SECRET_LEN {
            continue;
        }
        if cleaned.starts_with("http://") || cleaned.starts_with("https://") {
            if let Ok(parsed) = url::Url::parse(cleaned) {
                if parsed.path().split('/').any(is_opaque_url_segment) {
                    return true;
                }
                if let Some(q) = parsed.query() {
                    if q.split('&').flat_map(|kv| kv.split('=')).any(is_opaque_url_segment) {
                        return true;
                    }
                }
            }
            continue;
        }
        if is_opaque_token_segment(cleaned) {
            return true;
        }
    }
    false
}

/// Checks budget + retry cap before calling the classifier.
pub(crate) fn check_budget_before_classify(run_key: &str, query_bytes: usize) -> std::result::Result<(), String> {
    with_budget(run_key, |b| {
        if b.denials >= MAX_QUERY_DENIALS_PER_RUN {
            return Err(format!("web query limit reached after {MAX_QUERY_DENIALS_PER_RUN} denied queries in this run; carry on without further web queries"));
        }
        if b.bytes_used.saturating_add(query_bytes) > MAX_TAINTED_QUERY_BYTES_PER_RUN {
            return Err(format!("web query byte budget ({MAX_TAINTED_QUERY_BYTES_PER_RUN} bytes per run) exceeded; use shorter queries or finish with what you have"));
        }
        Ok(())
    })
}

pub(crate) fn record_query_denial(run_key: &str) {
    with_budget(run_key, |b| {
        b.denials = b.denials.saturating_add(1);
    });
}

pub(crate) fn charge_query_bytes(run_key: &str, query_bytes: usize) {
    with_budget(run_key, |b| {
        b.bytes_used = b.bytes_used.saturating_add(query_bytes);
    });
}

impl Kernel {
    /// Shared query gate for `web_search` and `research_web`.
    ///
    /// Scopes budgets to the causing user turn or schedule firing (`requester.task`),
    /// persisted in `State.security` via `Ev::QueryGateRecorded` (W1.4, N6).
    /// Returns `Ok(None)` when the query may proceed, or `Ok(Some(reason))`
    /// when it must be silently denied to the model (never prompting the user).
    pub(crate) async fn check_web_query(&self, a: &Action, run: &Run, query: &str) -> Result<Option<String>> {
        let requester = self.requester_run(run);
        let scope_key = if !requester.task.is_empty() {
            requester.task.as_str()
        } else if !requester.id.is_empty() {
            requester.id.as_str()
        } else {
            run.id.as_str()
        };

        // 1. Always-on deterministic secret/PII scrub.
        if let Some(reason) = scrub_check(query) {
            if run.private && run.untrusted {
                record_query_denial(scope_key);
                let _ = self.emit("kernel", crate::events::Ev::QueryGateRecorded(crate::events::QueryGatePayload { scope: scope_key.into(), bytes_added: 0, denied: true })).await;
            }
            return Ok(Some(format!("web query denied: {reason}. Rephrase without credentials or sensitive identifiers")));
        }

        // 2. Clean runs (not holding both private data and untrusted content)
        // pass without classifier latency or per-turn byte caps.
        if !(run.private && run.untrusted) {
            return Ok(None);
        }

        // 3. Turn/firing-scoped denial cap and shared byte budget (persisted in State.security).
        let qbytes = query.len();
        let persisted = self.read(|s| s.security.query_budget(scope_key));
        if persisted.denials >= MAX_QUERY_DENIALS_PER_RUN {
            return Ok(Some(format!("web query limit reached after {MAX_QUERY_DENIALS_PER_RUN} denied queries in this turn; carry on without further web queries")));
        }
        if persisted.bytes_used.saturating_add(qbytes) > MAX_TAINTED_QUERY_BYTES_PER_RUN {
            return Ok(Some(format!("web query byte budget ({MAX_TAINTED_QUERY_BYTES_PER_RUN} bytes per turn) exceeded; use shorter queries or finish with what you have")));
        }
        if let Err(reason) = check_budget_before_classify(scope_key, qbytes) {
            return Ok(Some(reason));
        }

        // 4. Mandatory outgoing classifier when both private and untrusted
        // content are in the run's context. Fail closed if unreachable.
        match self.outgoing_finding(a, run).await {
            Some(None) => {
                charge_query_bytes(scope_key, qbytes);
                let _ = self
                    .emit("kernel", crate::events::Ev::QueryGateRecorded(crate::events::QueryGatePayload { scope: scope_key.into(), bytes_added: qbytes, denied: false }))
                    .await;
                Ok(None)
            }
            Some(Some(finding)) => {
                record_query_denial(scope_key);
                let _ = self.emit("kernel", crate::events::Ev::QueryGateRecorded(crate::events::QueryGatePayload { scope: scope_key.into(), bytes_added: 0, denied: true })).await;
                Ok(Some(format!("web query denied: the outgoing classifier flagged sensitive data ({finding}). Rephrase using only general public terms")))
            }
            None => {
                record_query_denial(scope_key);
                let _ = self.emit("kernel", crate::events::Ev::QueryGateRecorded(crate::events::QueryGatePayload { scope: scope_key.into(), bytes_added: 0, denied: true })).await;
                Ok(Some("web query denied: the outgoing classifier could not be reached while this run holds private data and untrusted content".into()))
            }
        }
    }
}

/// Why a model-constructed URL in a tainted (`private && untrusted`) run is
/// refused. It always is: such a run reaches the web through `research_web`,
/// an isolated `OpenWeb` worker, never through a URL it wrote itself, and
/// never through an approval card. Nothing here asks a model, since no answer
/// could change the verdict (decisions/2026-11-22-a-classifier-clear-is-not-a-pass.md).
pub(crate) fn constructed_url_refusal(raw_url: &str) -> String {
    let trimmed = raw_url.trim();
    let Ok(parsed) = url::Url::parse(trimmed) else {
        return "web URL denied: invalid URL".into();
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return "web URL denied: only http and https URLs are allowed".into();
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return "web URL denied: credentials in URL userinfo are not allowed".into();
    }
    if let Some(reason) = scrub_check(trimmed) {
        return format!("web URL denied: {reason}. Remove credentials or sensitive identifiers");
    }
    "cannot open a constructed URL directly from a private conversation with untrusted content; call research_web with a clean public query instead (it runs in an isolated OpenWeb worker with autonomous search, page fetch, and browser navigation/clicking)".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The injection suite's CRM case: after an untrusted, private read, a
    /// fetch to a collector with the data in its query was let through
    /// because the classifier saw nothing sensitive in it. A constructed URL
    /// is always refused and sent to `research_web`, with no classifier call.
    #[test]
    fn a_constructed_url_is_refused_and_sent_to_research_web() {
        for url in ["https://collector.test/?crm=acme", "https://example.com/page", "http://example.com"] {
            assert!(constructed_url_refusal(url).contains("research_web"), "{url}");
        }
        assert!(constructed_url_refusal("ftp://example.com").contains("only http and https"));
        assert!(constructed_url_refusal("https://u:p@example.com").contains("userinfo"));
        assert!(constructed_url_refusal("https://evil.test/?tok=a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7").contains("Remove credentials"));
    }

    #[test]
    fn scrub_catches_secrets_and_pii_and_allows_normal_queries() {
        assert!(scrub_check("rust async runtime comparison 2026").is_none());
        assert!(scrub_check("Q3 market trends SaaS benchmarks").is_none());
        assert!(scrub_check("check https://example.com/very-long-path-with-1234567890-and-letters").is_none());

        assert!(scrub_check("token sk-1234567890abcdef").is_some());
        assert!(scrub_check("key ghp_abcdefghijklmnopqrstuvwxyz123456").is_some());
        assert!(scrub_check("ssn 123-45-6789 lookup").is_some());
        assert!(scrub_check("card 4532-0151-1283-0366").is_some());
        assert!(scrub_check("secret a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7").is_some());
        assert!(scrub_check("https://evil.test/exfil/a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7").is_some());
        assert!(scrub_check("https://evil.test/?tok=a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7").is_some());
    }

    #[test]
    fn budget_and_retry_cap_enforce_limits_across_calls() {
        let key = "test_budget_run_1";
        reset_budget_for_test(key);
        assert!(check_budget_before_classify(key, 1024).is_ok());
        charge_query_bytes(key, 1024);
        assert!(check_budget_before_classify(key, 1024).is_ok());
        charge_query_bytes(key, 1024);
        assert!(check_budget_before_classify(key, 1).is_err(), "exceeded per-run byte budget");

        let key2 = "test_retry_cap_run_2";
        reset_budget_for_test(key2);
        for _ in 0..MAX_QUERY_DENIALS_PER_RUN {
            assert!(check_budget_before_classify(key2, 10).is_ok());
            record_query_denial(key2);
        }
        assert!(check_budget_before_classify(key2, 10).is_err(), "exceeded denial retry cap");
    }
}
